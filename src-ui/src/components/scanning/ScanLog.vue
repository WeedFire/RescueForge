<script setup lang="ts">
import { Activity } from 'lucide-vue-next'

defineProps<{
  logs: string[]
}>()
</script>

<template>
  <div class="glass-card p-4 max-h-[500px] overflow-hidden flex flex-col">
    <h3 class="text-sm font-semibold text-[#94A3B8] mb-3 flex items-center gap-2">
      <Activity class="w-4 h-4" />
      实时日志
    </h3>
    <div class="flex-1 overflow-y-auto space-y-1 font-mono text-xs">
      <div
        v-for="(log, i) in logs"
        :key="i"
        class="text-[#94A3B8] hover:text-[#E2E8F0] transition-colors py-0.5"
      >
        {{ log }}
      </div>
      <div v-if="logs.length === 0" class="text-[#64748B] text-center py-8">
        暂无日志
      </div>
    </div>
  </div>
</template>
