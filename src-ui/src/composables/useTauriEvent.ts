import { onMounted, onUnmounted } from 'vue'
import { useRouter } from 'vue-router'
import { useScanStore } from '@/stores/scanStore'

/**
 * 全局扫描事件监听（由 AppShell 挂载，应用生命周期内常驻）。
 * 职责：更新 scanStore + 扫描完成后跳转结果页。
 * 注意：不要将此监听放到 Dashboard 等页面组件中——页面切换后监听从卸载，
 * 曾导致扫描完成后无人处理、进度卡 0 的竞态 Bug。
 */
export function useTauriEvent() {
  const scanStore = useScanStore()
  const router = useRouter()
  let unlisten: (() => void) | null = null

  async function startListening() {
    const { onScanEvent } = await import('@/api/tauri')
    try {
      unlisten = await onScanEvent((event) => {
        console.info('[scan-event]', event.status, 'progress=', event.progress,
          'files=', event.found_files, 'session=', event.session_id, event.message)
        scanStore.handleScanEvent(event)
        // 扫描完成：仅当用户停留在扫描页时跳转结果页
        if (event.status === 'completed' && router.currentRoute.value.path === '/scanning') {
          setTimeout(() => {
            if (router.currentRoute.value.path === '/scanning') {
              router.push('/results')
            }
          }, 1000)
        }
      })
      console.info('[scan-event] 监听器注册成功')
    } catch (e) {
      // 监听失败（如 Tauri 权限缺失）必须显式暴露，绝不能静默吞掉
      console.error('[scan-event] 监听器注册失败，扫描进度将无法更新:', e)
      scanStore.scanLog.push(`[${new Date().toLocaleTimeString()}] ❌ 事件监听注册失败: ${e}`)
    }
  }

  onMounted(() => {
    startListening()
  })

  onUnmounted(() => {
    if (unlisten) {
      unlisten()
    }
  })
}
