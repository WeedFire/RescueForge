<script setup lang="ts">
import { FolderTree } from 'lucide-vue-next'

defineProps<{
  files: { id: string; filename: string; extension: string }[]
}>()
</script>

<template>
  <div class="glass-card p-4 max-h-[500px] overflow-hidden flex flex-col">
    <h3 class="text-sm font-semibold text-[#94A3B8] mb-3 flex items-center gap-2">
      <FolderTree class="w-4 h-4" />
      文件树
    </h3>
    <div class="flex-1 overflow-y-auto">
      <div
        v-for="file in files.slice(0, 20)"
        :key="file.id"
        class="flex items-center gap-2 py-1.5 px-2 rounded hover:bg-[rgba(74,144,217,0.08)] cursor-pointer transition-colors"
      >
        <span class="text-xs text-[#4A90D9] font-mono">.{{ file.extension }}</span>
        <span class="text-xs text-[#E2E8F0] truncate">{{ file.filename }}</span>
      </div>
      <div v-if="files.length === 0" class="text-[#64748B] text-sm text-center py-8">
        扫描开始后将在此显示发现的文件
      </div>
    </div>
  </div>
</template>
