<script setup lang="ts">
import { Eye } from 'lucide-vue-next'
import type { RecoveredFile } from '@/types'

defineProps<{
  file: RecoveredFile | null
}>()
</script>

<template>
  <div class="glass-card p-4 min-h-[300px] flex flex-col">
    <h3 class="text-sm font-semibold text-[#94A3B8] mb-3 flex items-center gap-2">
      <Eye class="w-4 h-4" />
      预览
    </h3>
    <div class="flex-1 flex items-center justify-center">
      <div v-if="!file" class="text-center text-[#64748B]">
        <Eye class="w-12 h-12 mx-auto mb-2 opacity-30" />
        <p class="text-sm">选择文件以预览</p>
      </div>
      <div v-else-if="!file.preview_available" class="text-center text-[#64748B]">
        <p class="text-sm">暂不支持预览此文件类型</p>
      </div>
      <div v-else class="text-center text-[#64748B]">
        <p class="text-sm">文件预览功能将在后续实现</p>
      </div>
    </div>
  </div>
</template>
