<script setup lang="ts">
import { ShoppingCart } from 'lucide-vue-next'

defineProps<{
  fileCount: number
  totalSize: number
}>()

const emit = defineEmits<{
  'recover': []
  'clear': []
}>()
</script>

<template>
  <div
    v-if="fileCount > 0"
    class="fixed bottom-12 left-56 right-0 glass-card mx-6 px-6 py-3 flex items-center justify-between z-10 border-[rgba(0,255,136,0.3)]"
  >
    <div class="flex items-center gap-3">
      <ShoppingCart class="w-5 h-5 text-[#00FF88]" />
      <span class="text-sm text-[#E2E8F0]">
        已选: <strong class="text-[#00FF88]">{{ fileCount }}</strong> 个文件
      </span>
      <span class="text-sm text-[#94A3B8]">
        总大小: <strong>{{ (totalSize / 1024 / 1024).toFixed(1) }} MB</strong>
      </span>
    </div>
    <div class="flex items-center gap-3">
      <button @click="emit('clear')" class="btn-secondary text-xs py-1.5 px-4">
        清空
      </button>
      <button @click="emit('recover')" class="btn-primary text-xs py-1.5 px-6">
        一键恢复
      </button>
    </div>
  </div>
</template>
