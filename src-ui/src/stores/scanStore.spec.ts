import { describe, it, expect, beforeEach, vi } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import type { ScanEvent } from '@/types'

// startPathScan 内部会动态 import('@/api/tauri')，mock 掉避免真实 invoke。
// 注意：vi.mock 工厂会被提升到文件顶部，不能引用尚未初始化的 vi 之外的变量；
// 直接用普通 async 函数，避免工厂求值时机问题。
vi.mock('@/api/tauri', () => ({
  startPathScan: async () => 'ps-1',
  startScan: async () => 'scan-1',
  getScanResults: async () => ({ total: 0, offset: 0, files: [] }),
}))

import { useScanStore } from './scanStore'

function makeEvent(overrides: Partial<ScanEvent>): ScanEvent {
  return {
    status: 'scanning',
    progress: 0,
    message: '',
    ...overrides,
  } as ScanEvent
}

describe('scanStore 扫描事件竞态处理', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  it('当扫描事件先于会话注册到达时（小目录秒扫完），进度不应丢失', () => {
    const store = useScanStore()

    // 后端在 invoke 返回前就 emit 了完整扫描序列
    store.handleScanEvent(makeEvent({ session_id: 'ps-1', progress: 0, message: '开始扫描' }))
    store.handleScanEvent(makeEvent({ session_id: 'ps-1', progress: 1, found_files: 6, message: '完成' }))
    store.handleScanEvent(makeEvent({ session_id: 'ps-1', status: 'completed', progress: 1, found_files: 6, message: '完成!' }))

    const session = store.sessions.find(s => s.id === 'ps-1')
    expect(session, '事件应先懒创建会话').toBeTruthy()
    expect(session!.progress).toBe(1)
    expect(session!.status).toBe('completed')
    expect(store.isScanning).toBe(false)
  })

  it('startPathScan 返回时不应把已完成会话重置为"扫描中"', async () => {
    const store = useScanStore()

    // 事件先到达并已完成（会触发后台分页拉取的浮动 Promise）
    store.handleScanEvent(makeEvent({ session_id: 'ps-1', status: 'completed', progress: 1, found_files: 6 }))
    // 等浮动任务先落定，避免与下一次动态 import 产生竞态
    await new Promise(r => setTimeout(r, 0))

    await store.startPathScan(['D:\\temp'], 'deep')

    const session = store.sessions.find(s => s.id === 'ps-1')
    expect(session, '不应重复推入会话').toBeTruthy()
    expect(store.sessions.filter(s => s.id === 'ps-1').length).toBe(1)
    expect(session!.progress, '进度不应被重置为 0').toBe(1)
    expect(store.isScanning, '已完成的会话不应被标记为扫描中').toBe(false)
  })

  it('正常流程：会话注册后事件能正确更新进度', async () => {
    const store = useScanStore()
    await store.startPathScan(['D:\\temp'], 'deep')

    store.handleScanEvent(makeEvent({ session_id: 'ps-1', progress: 0.5, found_files: 3 }))

    expect(store.currentSession?.progress).toBe(0.5)
    expect(store.currentSession?.found_files).toBe(3)
    expect(store.isScanning).toBe(true)
  })
})
