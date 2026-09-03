<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import type { DiskInfo } from '@/types'
import SmartIndicator from './SmartIndicator.vue'
import { HardDrive, Search, ScanLine } from 'lucide-vue-next'

const { t } = useI18n()
const router = useRouter()
const props = defineProps<{ disk: DiskInfo }>()

function formatBytes(bytes: number): string {
  if (bytes >= 1e12) return `${(bytes / 1e12).toFixed(1)} TB`
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(1)} GB`
  return `${(bytes / 1e6).toFixed(0)} MB`
}

const usagePercent = computed(() => {
  // 真实使用率：由后端 free_bytes 计算（无数据时回退 0）
  const total = props.disk.capacity_bytes
  const free = props.disk.free_bytes ?? 0
  if (!total || total <= 0) return 0
  return Math.min(100, Math.max(0, Math.round((1 - free / total) * 100)))
})

function goScan(mode: string) {
  router.push(`/scanning?disk=${props.disk.index}&mode=${mode}`)
}
</script>

<template>
  <div class="card-interactive p-5 group">
    <!-- Header: icon + name + status -->
    <div class="flex items-start gap-4 mb-5">
      <div class="w-11 h-11 rounded-xl bg-gradient-to-br from-brand-blue/15 to-brand-cyan/10 flex items-center justify-center flex-shrink-0 ring-1 ring-brand-blue/20">
        <HardDrive class="w-5 h-5 text-brand-blue" />
      </div>
      <div class="flex-1 min-w-0">
        <div class="flex items-center justify-between gap-2">
          <h3 class="text-base font-semibold truncate">{{ disk.model || `磁盘 ${disk.index + 1}` }}</h3>
          <SmartIndicator
            v-if="disk.smart_status"
            :healthy="disk.smart_status.healthy"
            :warning="!!disk.smart_status.warning"
          />
        </div>
        <p class="text-xs text-t-muted font-mono truncate mt-0.5">{{ disk.device_path }}</p>
      </div>
    </div>

    <!-- Key Stats Row -->
    <div class="flex items-center gap-4 mb-4 px-1">
      <div class="flex-1">
        <p class="text-3xs text-t-muted uppercase tracking-wider font-semibold mb-1">容量</p>
        <p class="text-sm font-bold font-mono tracking-tight">{{ formatBytes(disk.capacity_bytes) }}</p>
      </div>
      <div class="w-px h-8 bg-app-border" />
      <div class="flex-1">
        <p class="text-3xs text-t-muted uppercase tracking-wider font-semibold mb-1">分区</p>
        <p class="text-sm font-bold font-mono tracking-tight">{{ disk.partitions.length }} 个</p>
      </div>
      <div v-if="disk.smart_status?.temperature !== undefined" class="w-px h-8 bg-app-border" />
      <div v-if="disk.smart_status?.temperature !== undefined" class="flex-1">
        <p class="text-3xs text-t-muted uppercase tracking-wider font-semibold mb-1">温度</p>
        <p class="text-sm font-bold font-mono tracking-tight">
          <span :class="disk.smart_status.temperature > 50 ? 'text-brand-red' : disk.smart_status.temperature > 40 ? 'text-brand-amber' : 'text-brand-green'">
            {{ disk.smart_status.temperature }}°C
          </span>
        </p>
      </div>
    </div>

    <!-- Usage Bar -->
    <div class="mb-5 px-1">
      <div class="flex justify-between mb-1.5">
        <span class="text-3xs text-t-muted font-medium">磁盘使用率</span>
        <span class="text-3xs text-t-secondary font-mono">{{ usagePercent }}%</span>
      </div>
      <div class="progress-track">
        <div
          class="progress-fill-blue"
          :style="{ width: usagePercent + '%' }"
        />
      </div>
    </div>

    <!-- Action Buttons -->
    <div class="flex gap-2.5">
      <button @click="goScan('quick')" class="btn-primary flex-1 gap-1.5 text-sm">
        <Search class="w-3.5 h-3.5" />
        快速恢复
      </button>
      <button @click="goScan('deep')" class="btn-secondary flex-1 gap-1.5 text-sm">
        <ScanLine class="w-3.5 h-3.5" />
        深度扫描
      </button>
    </div>
  </div>
</template>
