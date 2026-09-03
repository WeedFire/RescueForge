<script setup lang="ts">
import type { FoundPartition } from '@/types'
import { HardDrive, CheckCircle } from 'lucide-vue-next'

defineProps<{
  partition: FoundPartition
  selected: boolean
}>()

const emit = defineEmits<{
  'toggle': []
}>()
</script>

<template>
  <div
    @click="emit('toggle')"
    :class="[
      'glass-card p-4 cursor-pointer transition-all duration-200 flex items-center justify-between',
      selected
        ? 'border-[rgba(0,255,136,0.3)] shadow-[0_0_12px_rgba(0,255,136,0.1)]'
        : 'hover:border-[rgba(74,144,217,0.3)]'
    ]"
  >
    <div class="flex items-center gap-3">
      <HardDrive class="w-8 h-8 text-[#4A90D9]" />
      <div>
        <p class="text-sm font-semibold text-[#E2E8F0]">
          {{ partition.label || '未命名分区' }}
        </p>
        <p class="text-xs text-[#94A3B8]">
          {{ partition.filesystem }} · {{ (partition.size_sectors * 512 / 1e9).toFixed(1) }} GB
        </p>
        <p class="text-xs text-[#64748B]">
          可信度: {{ (partition.confidence * 100).toFixed(0) }}%
        </p>
      </div>
    </div>
    <CheckCircle
      v-if="selected"
      class="w-5 h-5 text-[#00FF88]"
    />
  </div>
</template>
