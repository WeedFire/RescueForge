<script setup lang="ts">
import { computed } from 'vue'

const props = defineProps<{
  healthy: boolean
  warning: boolean
}>()

const config = computed(() => {
  if (props.healthy && !props.warning) return { label: '良好', bg: 'bg-brand-green/12', text: 'text-brand-green', dot: 'bg-brand-green' }
  if (props.warning) return { label: '警告', bg: 'bg-brand-amber/12', text: 'text-brand-amber', dot: 'bg-brand-amber animate-pulse-dot' }
  return { label: '异常', bg: 'bg-brand-red/12', text: 'text-brand-red', dot: 'bg-brand-red animate-pulse-dot' }
})
</script>

<template>
  <span :class="['inline-flex items-center gap-1.5 px-2 py-1 rounded-md text-3xs font-semibold', config.bg, config.text]">
    <span :class="['w-1.5 h-1.5 rounded-full', config.dot]" />
    {{ config.label }}
  </span>
</template>
