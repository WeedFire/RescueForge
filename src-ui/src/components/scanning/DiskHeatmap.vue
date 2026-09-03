<script setup lang="ts">
import { onMounted, ref } from 'vue'

const canvasRef = ref<HTMLCanvasElement | null>(null)

defineProps<{
  progress: number
}>()

onMounted(() => {
  const canvas = canvasRef.value
  if (!canvas) return

  const ctx = canvas.getContext('2d')
  if (!ctx) return

  // Draw placeholder heatmap
  const w = canvas.width
  const h = canvas.height
  ctx.fillStyle = '#1A2332'
  ctx.fillRect(0, 0, w, h)

  // Grid pattern
  ctx.strokeStyle = '#243447'
  ctx.lineWidth = 0.5
  const cellSize = 8
  for (let x = 0; x < w; x += cellSize) {
    for (let y = 0; y < h; y += cellSize) {
      ctx.strokeRect(x, y, cellSize, cellSize)
    }
  }

  // Some "scanned" cells
  const scannedCells = Math.floor((0.3 * w * h) / (cellSize * cellSize))
  for (let i = 0; i < scannedCells; i++) {
    const cx = Math.floor(Math.random() * (w / cellSize)) * cellSize
    const cy = Math.floor(Math.random() * (h / cellSize)) * cellSize
    ctx.fillStyle = `rgba(74, 144, 217, ${0.3 + Math.random() * 0.5})`
    ctx.fillRect(cx + 1, cy + 1, cellSize - 1, cellSize - 1)
  }

  // "Found" gold particles
  for (let i = 0; i < 5; i++) {
    const px = Math.random() * w
    const py = Math.random() * h
    ctx.fillStyle = '#FFB800'
    ctx.beginPath()
    ctx.arc(px, py, 3, 0, Math.PI * 2)
    ctx.fill()
  }

  // Text overlay
  ctx.fillStyle = 'rgba(255, 255, 255, 0.8)'
  ctx.font = '14px "PingFang SC", sans-serif'
  ctx.textAlign = 'center'
  ctx.fillText('磁盘热力图', w / 2, h / 2)
})
</script>

<template>
  <div class="glass-card p-4">
    <h3 class="text-sm font-semibold text-[#94A3B8] mb-3">磁盘热力图</h3>
    <canvas
      ref="canvasRef"
      width="400"
      height="300"
      class="w-full rounded-lg border border-dark-600"
    />
  </div>
</template>
