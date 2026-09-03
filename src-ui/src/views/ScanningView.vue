<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { useRoute, useRouter } from 'vue-router'
import { useScanStore } from '@/stores/scanStore'
import { useScanControl } from '@/composables/useScanControl'
import { Pause, Play, Square, Search, FolderOpen } from 'lucide-vue-next'
import { computed, onMounted } from 'vue'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const scanStore = useScanStore()
const { pause, resume, cancel } = useScanControl()

// 从仪表盘进入：/scanning?disk=<index>&mode=<quick|deep|raw> → 自动启动磁盘扫描
onMounted(async () => {
  const diskParam = route.query.disk
  const modeParam = route.query.mode
  if (diskParam === undefined || diskParam === null) return
  const diskIndex = Number(diskParam)
  if (Number.isNaN(diskIndex)) return
  const mode = (modeParam === 'raw' || modeParam === 'quick' || modeParam === 'deep') ? modeParam : 'deep'
  // 已在扫描中则不重复启动（例如刷新页面）
  if (scanStore.isScanning) return
  try {
    scanStore.scanLog.push(`[${new Date().toLocaleTimeString()}] ▶ 启动磁盘 ${diskIndex} ${mode === 'deep' ? '深度' : mode === 'quick' ? '快速' : 'RAW'}扫描...`)
    await scanStore.startScan(diskIndex, mode)
    // 小磁盘可能在 invoke 返回前已完成：直接跳结果页
    if (scanStore.currentSession?.status === 'completed') {
      router.push('/results')
    }
  } catch (e) {
    console.error('[scan] startScan 失败:', e)
    scanStore.scanLog.push(`[${new Date().toLocaleTimeString()}] ❌ 启动扫描失败: ${e}`)
    alert(`启动扫描失败: ${e}`)
  }
})

const progressPercent = computed(() => {
  const s = scanStore.currentSession
  return s ? Math.round(s.progress * 100) : 0
})

const isPathScan = computed(() => {
  return scanStore.currentSession?.target_paths && scanStore.currentSession.target_paths.length > 0
})

const currentPathName = computed(() => {
  const p = scanStore.currentSession?.current_path
  if (!p) return ''
  const parts = p.replace(/\\/g, '/').split('/')
  return parts[parts.length - 1] || p
})
</script>

<template>
  <div class="p-6 space-y-6 max-w-[900px] mx-auto">
    <!-- Header -->
    <div class="flex items-center justify-between">
      <div>
        <h1 class="text-xl font-semibold tracking-tight">{{ t('scanning.title') }}</h1>
        <p class="text-sm text-t-secondary mt-0.5">
          {{ scanStore.isPaused ? '扫描已暂停' : scanStore.isScanning ? '扫描进行中' : '扫描结束' }}
        </p>
      </div>
      <div class="flex items-center gap-2">
        <button
          v-if="scanStore.isPaused"
          @click="resume()"
          class="btn-primary-sm gap-1.5"
        >
          <Play class="w-3 h-3" /> {{ t('scanning.resume') }}
        </button>
        <button
          v-else-if="scanStore.isScanning"
          @click="pause()"
          class="btn-secondary-sm gap-1.5"
        >
          <Pause class="w-3 h-3" /> {{ t('scanning.pause') }}
        </button>
        <button
          v-if="scanStore.isScanning"
          @click="cancel()"
          class="btn-danger-sm gap-1.5"
        >
          <Square class="w-3 h-3" /> {{ t('scanning.cancel') }}
        </button>
      </div>
    </div>

    <!-- Multi-path indicators (for folder scan) -->
    <div v-if="isPathScan && scanStore.currentSession?.target_paths" class="card p-4">
      <div class="flex items-center gap-2 mb-3">
        <FolderOpen class="w-4 h-4 text-brand-cyan" />
        <span class="text-xs font-medium text-t-secondary">
          {{ scanStore.currentSession.target_paths.length }} 个扫描路径
        </span>
      </div>
      <div class="space-y-2">
        <div
          v-for="(p, idx) in scanStore.currentSession.target_paths"
          :key="p"
          class="flex items-center gap-2 text-xs"
        >
          <div
            :class="[
              'w-5 h-5 rounded flex items-center justify-center flex-shrink-0 text-3xs font-bold',
              p === scanStore.currentSession.current_path
                ? 'bg-brand-blue text-white'
                : 'bg-app-input text-t-muted'
            ]"
          >
            {{ idx + 1 }}
          </div>
          <span
            :class="[
              'truncate font-mono',
              p === scanStore.currentSession.current_path ? 'text-t-primary' : 'text-t-muted'
            ]"
          >
            {{ p.replace(/\\/g, '/').split('/').pop() || p }}
          </span>
          <span
            v-if="p === scanStore.currentSession.current_path"
            class="text-brand-blue text-3xs animate-pulse"
          >
            扫描中...
          </span>
        </div>
      </div>
    </div>

    <!-- Overall Progress -->
    <div class="card p-5">
      <div class="flex items-center justify-between mb-3">
        <span class="text-sm font-medium">{{ t('scanning.progress') }}</span>
        <span class="text-sm font-mono text-brand-blue tabular-nums">{{ progressPercent }}%</span>
      </div>
      <div class="progress-track">
        <div
          class="progress-fill-blue"
          :style="{ width: progressPercent + '%' }"
        />
      </div>
      <div class="flex justify-between mt-3 text-xs text-t-muted">
        <span>发现文件: <span class="text-brand-green font-medium">{{ scanStore.currentSession?.found_files ?? 0 }}</span></span>
        <span v-if="isPathScan">
          当前: {{ currentPathName }}
        </span>
        <span v-if="scanStore.currentSession?.speed_mbps">
          速度: {{ scanStore.currentSession?.speed_mbps.toFixed(1) }} MB/s
        </span>
      </div>
    </div>

    <!-- Log -->
    <div class="card p-5">
      <h2 class="text-sm font-semibold mb-3">扫描日志</h2>
      <div v-if="scanStore.scanLog.length === 0" class="text-center py-8">
        <Search class="w-8 h-8 text-t-muted mx-auto mb-2" />
        <p class="text-xs text-t-muted">等待扫描开始...</p>
      </div>
      <div v-else class="bg-app-input rounded-md p-3 max-h-80 overflow-y-auto font-mono text-xs space-y-0.5">
        <p v-for="(line, i) in scanStore.scanLog" :key="i" class="text-t-secondary leading-relaxed">
          {{ line }}
        </p>
      </div>
    </div>
  </div>
</template>
