<script setup lang="ts">
import type { RecoveredFile } from '@/types'
import { File, Image, FileText, Film, Music, Archive } from 'lucide-vue-next'

defineProps<{
  files: RecoveredFile[]
  viewMode: 'grid' | 'list'
}>()

function getFileIcon(ext: string) {
  const imageExts = ['jpg', 'jpeg', 'png', 'gif', 'bmp', 'webp']
  const docExts = ['pdf', 'doc', 'docx', 'txt', 'md']
  const videoExts = ['mp4', 'avi', 'mkv', 'mov']
  const audioExts = ['mp3', 'wav', 'flac']
  const archiveExts = ['zip', 'rar', '7z']

  if (imageExts.includes(ext.toLowerCase())) return Image
  if (docExts.includes(ext.toLowerCase())) return FileText
  if (videoExts.includes(ext.toLowerCase())) return Film
  if (audioExts.includes(ext.toLowerCase())) return Music
  if (archiveExts.includes(ext.toLowerCase())) return Archive
  return File
}
</script>

<template>
  <div :class="[
    viewMode === 'grid'
      ? 'grid grid-cols-2 md:grid-cols-4 lg:grid-cols-6 gap-3'
      : 'space-y-1'
  ]">
    <div
      v-for="file in files"
      :key="file.id"
      class="glass-card p-3 cursor-pointer transition-all duration-200 hover:border-[rgba(74,144,217,0.4)]"
    >
      <div class="flex items-center gap-2 mb-2">
        <component :is="getFileIcon(file.extension)" class="w-5 h-5 text-[#4A90D9]" />
        <span class="text-xs text-[#64748B] font-mono">.{{ file.extension }}</span>
      </div>
      <p class="text-xs text-[#E2E8F0] truncate mb-1">{{ file.filename }}</p>
      <p class="text-xs text-[#94A3B8]">{{ (file.size_bytes / 1024).toFixed(1) }} KB</p>
    </div>
  </div>
</template>
