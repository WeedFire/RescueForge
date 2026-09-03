<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { useDiskStore } from '@/stores/diskStore'
import { useScanStore } from '@/stores/scanStore'
import { HardDrive, Clock, RefreshCw, ChevronRight } from 'lucide-vue-next'
import DiskCard from '@/components/dashboard/DiskCard.vue'
import PathManager from '@/components/dashboard/PathManager.vue'

const { t } = useI18n()
const router = useRouter()
const diskStore = useDiskStore()
const scanStore = useScanStore()

// Tab state: 'disks' | 'folders'
const activeTab = ref<'disks' | 'folders'>('folders')

// 扫描事件由 AppShell 的 useTauriEvent 全局监听（含完成后的结果页跳转），
// 页面组件不再重复注册，避免卸载后无人处理的竞态。
onMounted(() => {
  diskStore.fetchDisks()
})

async function handleFolderScan(paths: string[]) {
  try {
    await scanStore.startPathScan(paths, 'deep')
  } catch (e) {
    // 启动失败必须可见（例如后端未编译最新代码/权限缺失），绝不能静默吞掉
    console.error('[scan] startPathScan 失败:', e)
    scanStore.scanLog.push(`[${new Date().toLocaleTimeString()}] ❌ 启动扫描失败: ${e}`)
    alert(`启动扫描失败: ${e}`)
    return
  }
  // 小目录可能在 invoke 返回前已扫完：按最终状态决定去向
  if (scanStore.currentSession?.status === 'completed') {
    router.push('/results')
  } else {
    router.push('/scanning')
  }
}
</script>

<template>
  <div class="p-6 space-y-6 max-w-[1200px] mx-auto">
    <!-- Page Header -->
    <div class="flex items-center justify-between">
      <div>
        <h1 class="text-2xl font-bold tracking-tight">{{ t('dashboard.title') }}</h1>
        <p class="text-sm text-t-secondary mt-1">监控存储设备健康状态，执行数据恢复操作</p>
      </div>
      <button
        @click="diskStore.fetchDisks()"
        class="btn-ghost-sm gap-1.5"
        :disabled="diskStore.loading"
      >
        <RefreshCw :class="['w-3.5 h-3.5', diskStore.loading && 'animate-spin-slow']" />
        刷新
      </button>
    </div>

    <!-- Tab Switcher -->
    <div class="flex gap-1 p-1 rounded-lg bg-app-card border border-app-border w-fit">
      <button
        @click="activeTab = 'folders'"
        :class="[
          'px-4 py-2 rounded-md text-sm font-medium transition-all',
          activeTab === 'folders'
            ? 'bg-brand-blue text-white shadow-sm'
            : 'text-t-secondary hover:text-t-primary'
        ]"
      >
        📁 文件夹扫描
      </button>
      <button
        @click="activeTab = 'disks'"
        :class="[
          'px-4 py-2 rounded-md text-sm font-medium transition-all',
          activeTab === 'disks'
            ? 'bg-brand-blue text-white shadow-sm'
            : 'text-t-secondary hover:text-t-primary'
        ]"
      >
        💾 磁盘扫描
      </button>
    </div>

    <!-- Folder Scan Tab -->
    <div v-if="activeTab === 'folders'">
      <PathManager
        :is-scanning="scanStore.isScanning"
        @start-scan="handleFolderScan"
      />

      <!-- Current scan status banner -->
      <div v-if="scanStore.isScanning && scanStore.currentSession" class="mt-4 card p-4">
        <div class="flex items-center gap-3">
          <div class="w-8 h-8 border-2 border-app-border border-t-brand-blue rounded-full animate-spin-slow flex-shrink-0" />
          <div class="flex-1 min-w-0">
            <p class="text-sm font-medium truncate">
              {{ scanStore.currentSession.current_path || '扫描中...' }}
            </p>
            <div class="flex items-center gap-3 mt-1">
              <div class="flex-1 progress-track h-1">
                <div
                  class="progress-fill-blue h-full"
                  :style="{ width: `${Math.round(scanStore.currentSession.progress * 100)}%` }"
                />
              </div>
              <span class="text-2xs text-t-muted tabular-nums">
                {{ Math.round(scanStore.currentSession.progress * 100) }}%
              </span>
            </div>
          </div>
          <span class="text-xs text-brand-blue font-medium tabular-nums">
            {{ scanStore.currentSession.found_files }} 文件
          </span>
        </div>
      </div>
    </div>

    <!-- Disk Scan Tab -->
    <div v-if="activeTab === 'disks'">
      <!-- Loading State -->
      <div v-if="diskStore.loading" class="flex items-center justify-center py-32">
        <div class="flex flex-col items-center gap-4">
          <div class="relative">
            <div class="w-12 h-12 border-2 border-app-border rounded-full" />
            <div class="absolute inset-0 w-12 h-12 border-2 border-transparent border-t-brand-blue rounded-full animate-spin-slow" />
          </div>
          <p class="text-sm text-t-secondary">正在检测磁盘设备...</p>
        </div>
      </div>

      <!-- Error State -->
      <div v-else-if="diskStore.error" class="card p-8 text-center">
        <div class="w-14 h-14 rounded-2xl bg-brand-red/10 flex items-center justify-center mx-auto mb-4">
          <HardDrive class="w-7 h-7 text-brand-red" />
        </div>
        <h2 class="text-lg font-semibold text-brand-red mb-2">磁盘检测失败</h2>
        <p class="text-sm text-t-secondary max-w-md mx-auto">{{ diskStore.error }}</p>
        <button @click="diskStore.fetchDisks()" class="btn-secondary mt-4">重试</button>
      </div>

      <!-- Empty State -->
      <div v-else-if="diskStore.disks.length === 0" class="card p-12 text-center">
        <div class="w-16 h-16 rounded-2xl bg-app-input flex items-center justify-center mx-auto mb-5">
          <HardDrive class="w-8 h-8 text-t-muted" />
        </div>
        <h2 class="text-lg font-semibold mb-1">未检测到磁盘</h2>
        <p class="text-sm text-t-secondary max-w-sm mx-auto">请连接存储设备后刷新页面。支持本地硬盘、移动硬盘、U盘等设备。</p>
      </div>

      <!-- Disks Grid -->
      <div v-else class="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-4">
        <DiskCard
          v-for="disk in diskStore.disks"
          :key="disk.index"
          :disk="disk"
        />
      </div>
    </div>

    <!-- Scan History -->
    <section class="card">
      <div class="flex items-center justify-between px-5 py-4 border-b border-app-border">
        <div class="flex items-center gap-2.5">
          <div class="w-8 h-8 rounded-lg bg-brand-purple/10 flex items-center justify-center">
            <Clock class="w-4 h-4 text-brand-purple" />
          </div>
          <div>
            <h2 class="text-sm font-semibold">{{ t('dashboard.scanHistory') }}</h2>
            <p class="text-3xs text-t-muted">最近的扫描记录</p>
          </div>
        </div>
        <button class="btn-ghost-sm">
          查看全部 <ChevronRight class="w-3.5 h-3.5" />
        </button>
      </div>
      <div class="p-8 text-center">
        <p class="text-sm text-t-secondary">{{ t('dashboard.noHistory') }}</p>
        <p class="text-xs text-t-muted mt-1">执行磁盘扫描后，记录将显示在这里</p>
      </div>
    </section>
  </div>
</template>
