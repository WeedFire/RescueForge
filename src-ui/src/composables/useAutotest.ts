// 自动冒烟测试：环境变量 RF_AUTOTEST=1 启动时自动跑全链路
// 真实 invoke + 真实 listen + 真实恢复，100% 生产代码路径（含真实 ACL）。
// 每一步写后端日志 + 控制台，结果暴露在响应式状态中供 UI 展示。
import { reactive } from 'vue'
import {
  getAutotestFlag,
  writeAppLog,
  startPathScan,
  recoverFiles,
  onScanEvent,
} from '@/api/tauri'
import type { ScanEvent, FoundFileInfo } from '@/types'

export interface AutotestState {
  enabled: boolean
  running: boolean
  finished: boolean
  passed: boolean
  logs: string[]
  foundFiles: FoundFileInfo[]
  recoverResult: { success: number; failed: number } | null
  outputPath: string
}

const SCAN_PATH = 'D:\\temp'
const OUTPUT_PATH = 'D:\\temp_autotest_recovered'
const TIMEOUT_MS = 120_000

export function useAutotest() {
  const state = reactive<AutotestState>({
    enabled: false,
    running: false,
    finished: false,
    passed: false,
    logs: [],
    foundFiles: [],
    recoverResult: null,
    outputPath: OUTPUT_PATH,
  })

  function log(level: string, msg: string) {
    const line = `[AUTOTEST] [${level}] ${msg}`
    state.logs.push(line)
    // eslint-disable-next-line no-console
    console.log(line)
    writeAppLog(level, line).catch(() => {})
  }

  async function maybeRun(): Promise<void> {
    let flag = false
    try {
      flag = await getAutotestFlag()
    } catch (e) {
      // eslint-disable-next-line no-console
      console.error('[AUTOTEST] get_autotest_flag 失败（IPC/ACL 问题）:', e)
      return
    }
    if (!flag) return
    state.enabled = true
    state.running = true
    await runSmoke()
  }

  async function runSmoke(): Promise<void> {
    log('INFO', `冒烟开始: 扫描 ${SCAN_PATH}`)
    // 用对象持有，避免 TS 控制流把闭包内赋值的变量收窄为 never
    const guard: { unlisten?: () => void } = {}
    try {
      // 1. 先注册事件监听（生产同款），再发起扫描，等 completed
      const completed = await new Promise<ScanEvent>((resolve, reject) => {
        const timer = setTimeout(() => {
          reject(new Error(`超时 ${TIMEOUT_MS}ms 未收到 completed 事件`))
        }, TIMEOUT_MS)

        onScanEvent((ev) => {
          log('INFO', `scan-event: status=${ev.status} progress=${ev.progress} found=${ev.found_files ?? '-'} msg=${ev.message}`)
          if (ev.result_files && ev.result_files.length > 0) {
            state.foundFiles = ev.result_files
          }
          if (ev.status === 'completed' || ev.status === 'error' || ev.status === 'cancelled') {
            clearTimeout(timer)
            // 延迟一拍再 resolve，确保 unlisten 已注册，避免竞态泄漏监听
            setTimeout(() => resolve(ev), 0)
          }
        })
          .then(async (fn) => {
            guard.unlisten = fn
            try {
              const sid = await startPathScan([SCAN_PATH], 'quick')
              log('INFO', `start_path_scan 返回，会话: ${sid}`)
            } catch (e) {
              clearTimeout(timer)
              reject(e instanceof Error ? e : new Error(String(e)))
            }
          })
          .catch((e) => { clearTimeout(timer); reject(e) })
      })

      if (completed.status !== 'completed') {
        throw new Error(`扫描异常结束: ${completed.status} - ${completed.message}`)
      }
      const files = completed.result_files ?? state.foundFiles
      const existing = files.filter((f) => !f.deleted)
      const deleted = files.filter((f) => f.deleted)
      log('INFO', `扫描完成: ${existing.length} 个现存文件, ${deleted.length} 个可恢复的已删除文件`)
      if (deleted.length > 0) {
        for (const d of deleted.slice(0, 5)) {
          log('INFO', `  [已删除] ${d.filename} (${d.size_bytes}B) ${d.filepath}`)
        }
      }
      if (existing.length === 0) {
        throw new Error('扫描完成但未发现任何现存文件')
      }
      state.foundFiles = files

      // 2. 恢复：后端约定 id||filepath 编码；冒烟只恢复现存文件（已删除文件需管理员卷访问，
      //    非提权终端下会失败，不纳入冒烟通过标准）
      const fileIds = existing.map((f) => `${f.id}||${f.filepath}`)
      log('INFO', `开始恢复 ${fileIds.length} 个文件到 ${OUTPUT_PATH}`)
      const res = await recoverFiles(fileIds, OUTPUT_PATH)
      state.recoverResult = res
      log('INFO', `恢复结束: success=${res.success} failed=${res.failed}`)

      state.passed = res.success > 0 && res.failed === 0
      log(state.passed ? 'INFO' : 'ERROR', state.passed
        ? `冒烟通过: 扫描 ${files.length} 个文件，恢复成功 ${res.success}`
        : `冒烟失败: success=${res.success} failed=${res.failed}`)
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      log('ERROR', `冒烟失败: ${msg}`)
      state.passed = false
    } finally {
      if (guard.unlisten) guard.unlisten()
      state.running = false
      state.finished = true
    }
  }

  return { state, maybeRun }
}
