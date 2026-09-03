<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { useDiskStore } from '@/stores/diskStore'
import {
  searchLostPartitions, rebuildPartitionTable, backupBootSector,
  restoreBootSector, createDiskImage, cancelDiskImage, onImageProgress,
  selectFolder,
} from '@/api/tauri'
import type { FoundPartition, ImageProgress } from '@/types'
import { Wrench, AlertTriangle, Search, RefreshCw, ShieldCheck, Save, Archive } from 'lucide-vue-next'

const { t } = useI18n()
const diskStore = useDiskStore()

// ===== 状态 =====
const selectedDisk = ref<number | null>(null)
const searching = ref(false)
const found = ref<FoundPartition[]>([])
const selected = ref<Set<number>>(new Set())
const msg = ref<{ type: 'ok' | 'err'; text: string } | null>(null)
const busy = ref(false)

// 引导扇区恢复
const lastBootBackup = ref('')
const restorePath = ref('')

// 磁盘镜像
const skipBad = ref(true)
const imaging = ref(false)
const imageProgress = ref<ImageProgress | null>(null)
let unlisten: (() => void) | null = null

function formatBytes(bytes: number): string {
  if (bytes >= 1e12) return `${(bytes / 1e12).toFixed(1)} TB`
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(1)} GB`
  return `${(bytes / 1e6).toFixed(0)} MB`
}

function setMsg(type: 'ok' | 'err', text: string) {
  msg.value = { type, text }
}

async function doSearch() {
  if (selectedDisk.value === null) return setMsg('err', '请先选择磁盘')
  searching.value = true
  found.value = []
  selected.value = new Set()
  msg.value = null
  try {
    found.value = await searchLostPartitions(selectedDisk.value)
    // 默认全选
    selected.value = new Set(found.value.map((_, i) => i))
    setMsg('ok', found.value.length > 0
      ? `找到 ${found.value.length} 个分区（表内 + 签名扫描候选）`
      : '未找到任何分区结构')
  } catch (e) {
    setMsg('err', `${e}`)
  } finally {
    searching.value = false
  }
}

function toggleSelect(i: number) {
  const s = new Set(selected.value)
  if (s.has(i)) s.delete(i); else s.add(i)
  selected.value = s
}

const selectedPartitions = computed(() =>
  found.value.filter((_, i) => selected.value.has(i))
)

async function doRebuild() {
  if (selectedDisk.value === null || selectedPartitions.value.length === 0) return
  if (!confirm(`确认将 ${selectedPartitions.value.length} 个分区写入磁盘 ${selectedDisk.value} 的 MBR 分区表？\n写入前会自动备份原分区表，但此操作仍有风险！`)) return
  busy.value = true
  try {
    const result = await rebuildPartitionTable(selectedDisk.value, selectedPartitions.value)
    setMsg('ok', result)
  } catch (e) {
    setMsg('err', `${e}`)
  } finally {
    busy.value = false
  }
}

async function doBackupBoot() {
  if (selectedDisk.value === null) return setMsg('err', '请先选择磁盘')
  busy.value = true
  try {
    const result = await backupBootSector(selectedDisk.value, 0)
    lastBootBackup.value = result.split(': ').pop() || ''
    restorePath.value = lastBootBackup.value
    setMsg('ok', result)
  } catch (e) {
    setMsg('err', `${e}`)
  } finally {
    busy.value = false
  }
}

async function doRestoreBoot() {
  if (selectedDisk.value === null) return setMsg('err', '请先选择磁盘')
  if (!restorePath.value.trim()) return setMsg('err', '请输入备份文件路径（512 字节的 .bin 文件）')
  if (!confirm('确认将备份的引导扇区写回该卷？此操作会覆盖当前引导扇区！')) return
  busy.value = true
  try {
    const result = await restoreBootSector(selectedDisk.value, 0, restorePath.value.trim())
    setMsg('ok', result)
  } catch (e) {
    setMsg('err', `${e}`)
  } finally {
    busy.value = false
  }
}

async function doImage() {
  if (selectedDisk.value === null) return setMsg('err', '请先选择磁盘')
  try {
    const folder = await selectFolder()
    if (!folder) return
    const outPath = `${folder}\\disk_${selectedDisk.value}_image.img`
    imaging.value = true
    imageProgress.value = null
    const result = await createDiskImage(selectedDisk.value, outPath, skipBad.value)
    setMsg('ok', result)
  } catch (e) {
    setMsg('err', `${e}`)
    imaging.value = false
  }
}

async function doCancelImage() {
  try { await cancelDiskImage() } catch (e) { setMsg('err', `${e}`) }
}

function handleProgress(p: ImageProgress) {
  imageProgress.value = p
  if (p.status === 'completed' || p.status === 'error') {
    imaging.value = false
    if (p.status === 'completed') setMsg('ok', `磁盘镜像完成，共 ${p.bad_sectors} 个坏扇区`)
  }
}

const imagePercent = computed(() => {
  const p = imageProgress.value
  if (!p || !p.total_sectors) return 0
  return Math.min(100, Math.round((p.copied_sectors / p.total_sectors) * 100))
})

onMounted(async () => {
  if (diskStore.disks.length === 0) await diskStore.fetchDisks()
  unlisten = await onImageProgress(handleProgress)
})

onUnmounted(() => {
  unlisten?.()
})
</script>

<template>
  <div class="p-6 space-y-6 max-w-[900px] mx-auto">
    <div>
      <h1 class="text-xl font-semibold tracking-tight">{{ t('partition.title') }}</h1>
      <p class="text-sm text-t-secondary mt-0.5">搜索丢失分区、重建分区表、备份/恢复引导扇区、创建磁盘镜像</p>
    </div>

    <!-- 警告 -->
    <div class="card border-brand-amber/25 bg-brand-amber/5 p-4 flex items-start gap-3">
      <AlertTriangle class="w-4 h-4 text-brand-amber flex-shrink-0 mt-0.5" />
      <div>
        <p class="text-sm font-medium text-brand-amber">{{ t('common.warning') }}</p>
        <p class="text-xs text-t-secondary mt-0.5">{{ t('partition.warning') }}重建/恢复操作需要管理员权限。</p>
      </div>
    </div>

    <!-- 步骤1：选择磁盘 -->
    <div class="card p-5">
      <div class="flex items-center gap-3 mb-4">
        <div class="w-6 h-6 rounded-full bg-brand-blue text-white text-xs flex items-center justify-center font-semibold">1</div>
        <p class="text-sm font-medium">{{ t('partition.step1') }}</p>
        <button @click="diskStore.fetchDisks()" class="btn-ghost-sm ml-auto">
          <RefreshCw :class="['w-3.5 h-3.5', diskStore.loading && 'animate-spin-slow']" />
        </button>
      </div>
      <div v-if="diskStore.loading" class="text-xs text-t-muted py-2">正在检测磁盘设备...</div>
      <div v-else-if="diskStore.error" class="text-xs text-brand-red py-2">{{ diskStore.error }}</div>
      <div v-else-if="diskStore.disks.length === 0" class="text-xs text-t-muted py-2">未检测到磁盘</div>
      <div v-else class="space-y-2">
        <button
          v-for="disk in diskStore.disks"
          :key="disk.index"
          @click="selectedDisk = disk.index"
          :class="[
            'w-full flex items-center gap-3 p-3 rounded-lg border text-left transition-all',
            selectedDisk === disk.index
              ? 'border-brand-blue ring-1 ring-brand-blue bg-app-card-hover'
              : 'border-app-border hover:border-app-border-light'
          ]"
        >
          <Wrench class="w-4 h-4 text-t-muted flex-shrink-0" />
          <div class="flex-1 min-w-0">
            <p class="text-sm font-medium truncate">{{ disk.model || `磁盘 ${disk.index}` }}</p>
            <p class="text-2xs text-t-muted font-mono truncate">{{ disk.device_path }}</p>
          </div>
          <span class="text-xs text-t-secondary font-mono flex-shrink-0">{{ formatBytes(disk.capacity_bytes) }}</span>
        </button>
      </div>
    </div>

    <!-- 步骤2：搜索丢失分区 -->
    <div class="card p-5">
      <div class="flex items-center gap-3 mb-4">
        <div class="w-6 h-6 rounded-full bg-brand-blue text-white text-xs flex items-center justify-center font-semibold">2</div>
        <p class="text-sm font-medium">{{ t('partition.step2') }}</p>
        <button
          @click="doSearch"
          class="btn-primary-sm ml-auto"
          :disabled="selectedDisk === null || searching || busy"
        >
          <Search v-if="!searching" class="w-3 h-3" />
          <RefreshCw v-else class="w-3 h-3 animate-spin-slow" />
          {{ searching ? t('partition.searching') : t('partition.searchPartitions') }}
        </button>
      </div>

      <!-- 结果表 -->
      <div v-if="found.length > 0" class="border border-app-border rounded-lg overflow-hidden">
        <table class="w-full text-xs">
          <thead>
            <tr class="bg-app-input text-t-secondary">
              <th class="px-3 py-2 text-left font-medium w-8"></th>
              <th class="px-3 py-2 text-left font-medium">起始扇区</th>
              <th class="px-3 py-2 text-left font-medium">大小</th>
              <th class="px-3 py-2 text-left font-medium">{{ t('partition.filesystem') }}</th>
              <th class="px-3 py-2 text-left font-medium">卷标</th>
              <th class="px-3 py-2 text-left font-medium">{{ t('partition.confidence') }}</th>
            </tr>
          </thead>
          <tbody class="divide-y divide-app-border">
            <tr
              v-for="(p, i) in found"
              :key="i"
              @click="toggleSelect(i)"
              :class="['cursor-pointer transition-colors', selected.has(i) ? 'bg-brand-blue/8' : 'hover:bg-app-card-hover']"
            >
              <td class="px-3 py-2">
                <input type="checkbox" :checked="selected.has(i)" @click.stop="toggleSelect(i)" class="accent-brand-blue" />
              </td>
              <td class="px-3 py-2 font-mono">{{ p.start_sector }}</td>
              <td class="px-3 py-2 font-mono">{{ formatBytes(p.size_sectors * 512) }}</td>
              <td class="px-3 py-2">{{ p.filesystem }}</td>
              <td class="px-3 py-2">{{ p.label || '-' }}</td>
              <td class="px-3 py-2">
                <span :class="p.confidence >= 1 ? 'badge-green' : 'badge-amber'">{{ Math.round(p.confidence * 100) }}%</span>
              </td>
            </tr>
          </tbody>
        </table>
      </div>
      <p v-else-if="!searching && msg" class="text-xs text-t-muted">搜索后结果将显示在这里</p>

      <!-- 重建按钮 -->
      <div v-if="found.length > 0" class="mt-4 flex items-center gap-3">
        <button
          @click="doRebuild"
          class="btn-primary"
          :disabled="selectedPartitions.length === 0 || busy"
        >
          <ShieldCheck class="w-4 h-4" />
          {{ t('partition.rebuild') }}（{{ selectedPartitions.length }} 个分区）
        </button>
        <span class="text-2xs text-t-muted">写入前自动备份原 MBR 到 %USERPROFILE%\RescueForgeBackups</span>
      </div>
    </div>

    <!-- 步骤3：引导扇区 -->
    <div class="card p-5">
      <div class="flex items-center gap-3 mb-4">
        <div class="w-6 h-6 rounded-full bg-brand-blue text-white text-xs flex items-center justify-center font-semibold">3</div>
        <p class="text-sm font-medium">引导扇区备份 / 恢复</p>
      </div>
      <div class="flex items-center gap-3 mb-3">
        <button @click="doBackupBoot" class="btn-secondary" :disabled="selectedDisk === null || busy">
          <Save class="w-4 h-4" />
          {{ t('partition.backupBoot') }}
        </button>
        <span class="text-2xs text-t-muted">备份卷引导扇区（512 字节）到备份目录</span>
      </div>
      <div class="flex items-center gap-2">
        <input v-model="restorePath" class="input flex-1 font-mono text-xs" placeholder="备份文件路径（.bin，512 字节）" />
        <button @click="doRestoreBoot" class="btn-danger" :disabled="selectedDisk === null || busy || !restorePath.trim()">
          {{ t('partition.restoreBoot') }}
        </button>
      </div>
    </div>

    <!-- 步骤4：磁盘镜像 -->
    <div class="card p-5">
      <div class="flex items-center gap-3 mb-4">
        <div class="w-6 h-6 rounded-full bg-brand-blue text-white text-xs flex items-center justify-center font-semibold">4</div>
        <p class="text-sm font-medium">创建磁盘镜像</p>
      </div>
      <div class="flex items-center gap-3 flex-wrap">
        <button @click="doImage" class="btn-primary" :disabled="selectedDisk === null || imaging || busy">
          <Archive class="w-4 h-4" />
          {{ imaging ? '镜像中...' : '选择保存位置并创建镜像' }}
        </button>
        <label class="flex items-center gap-2 text-xs text-t-secondary cursor-pointer">
          <input type="checkbox" v-model="skipBad" class="accent-brand-blue" :disabled="imaging" />
          跳过坏扇区（零填充）
        </label>
        <button v-if="imaging" @click="doCancelImage" class="btn-danger ml-auto">取消镜像</button>
      </div>
      <div v-if="imageProgress" class="mt-4">
        <div class="flex justify-between mb-1.5 text-xs">
          <span class="text-t-muted">镜像进度</span>
          <span class="text-t-secondary font-mono">
            {{ imagePercent }}% · {{ imageProgress.speed_mbps.toFixed(1) }} MB/s
            <template v-if="imageProgress.bad_sectors > 0"> · 坏扇区 {{ imageProgress.bad_sectors }}</template>
          </span>
        </div>
        <div class="progress-track">
          <div class="progress-fill-blue" :style="{ width: imagePercent + '%' }" />
        </div>
      </div>
    </div>

    <!-- 消息区 -->
    <div
      v-if="msg"
      :class="[
        'card p-4 text-sm flex items-start gap-2',
        msg.type === 'ok' ? 'border-brand-green/30 bg-brand-green/8' : 'border-brand-red/30 bg-brand-red/8'
      ]"
    >
      <span :class="msg.type === 'ok' ? 'text-brand-green' : 'text-brand-red'" class="flex-shrink-0 mt-0.5">
        {{ msg.type === 'ok' ? '✓' : '✗' }}
      </span>
      <p class="text-t-secondary break-all">{{ msg.text }}</p>
    </div>
  </div>
</template>
