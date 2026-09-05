<script setup lang="ts">
import { useScanStore } from '@/stores/scanStore'
import { computed, onMounted, ref } from 'vue'

const scanStore = useScanStore()

const scanStatus = computed(() => {
  if (scanStore.isScanning) return scanStore.isPaused ? 'paused' : 'scanning'
  return 'idle'
})

// 版本号从 Rust 后端动态读取，避免前端多处硬编码。
// dev（浏览器）环境无 Tauri 运行时则回退到编译期注入的 __APP_VERSION__（= package.json version）。
const version = ref(__APP_VERSION__)
onMounted(async () => {
  try {
    const { getVersion } = await import('@tauri-apps/api/app')
    version.value = await getVersion()
  } catch {
    /* 非 Tauri 环境，保持 __APP_VERSION__ */
  }
})
</script>

<template>
  <footer class="flex items-center justify-between h-7 px-4 bg-app-sidebar border-t border-app-border text-3xs text-t-muted select-none">
    <div class="flex items-center gap-3">
      <span class="font-semibold text-t-secondary">数据恢复</span>
      <span class="text-t-muted">v{{ version }}</span>
    </div>
    <div class="flex items-center gap-3">
      <span class="w-1 h-1 rounded-full"
        :class="{
          'bg-brand-green': scanStatus === 'scanning',
          'bg-brand-amber': scanStatus === 'paused',
          'bg-t-muted': scanStatus === 'idle'
        }"
      />
      <span>
        {{ scanStatus === 'scanning' ? '扫描中' : scanStatus === 'paused' ? '已暂停' : '就绪' }}
      </span>
      <template v-if="scanStore.isScanning && scanStore.currentSession">
        <span class="text-app-border">|</span>
        <span>{{ Math.round(scanStore.currentSession.progress * 100) }}% · {{ scanStore.currentSession.found_files }} 文件</span>
      </template>
    </div>
  </footer>
</template>
