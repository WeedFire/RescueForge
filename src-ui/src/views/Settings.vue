<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { useSettingsStore } from '@/stores/settingsStore'
import { selectFolder } from '@/api/tauri'
import { Leaf, Moon, Sun, Languages, FolderOpen, Info, Check, Download, Loader } from 'lucide-vue-next'
import type { ThemeMode } from '@/types'

const { t } = useI18n()
const settingsStore = useSettingsStore()
const saved = ref(false)

// 版本号从 Rust 后端动态读取（与打包版本一致），dev 环境回退编译期注入的 __APP_VERSION__
const appVersion = ref(__APP_VERSION__)
onMounted(async () => {
  try {
    const { getVersion } = await import('@tauri-apps/api/app')
    appVersion.value = await getVersion()
  } catch {
    /* 非 Tauri 环境，保持 __APP_VERSION__ */
  }
})


interface ThemeOption {
  key: ThemeMode
  label: string
  desc: string
  icon: typeof Sun
  swatch: string
}

const themeOptions: ThemeOption[] = [
  { key: 'eye', label: '护眼色', desc: '柔和豆绿，默认推荐', icon: Leaf, swatch: '#EAF2E3' },
  { key: 'dark', label: '深色', desc: '专业恢复工具风', icon: Moon, swatch: '#0D1117' },
  { key: 'light', label: '浅色', desc: '清爽灰白', icon: Sun, swatch: '#F5F7FA' },
]

async function browsePath() {
  try {
    const path = await selectFolder()
    if (path) {
      settingsStore.settings.default_recovery_path = path
    }
  } catch (e) {
    alert(`选择文件夹失败: ${e}`)
  }
}

async function save() {
  await settingsStore.saveSettings()
  saved.value = true
  setTimeout(() => { saved.value = false }, 2000)
}

// ===== 检查更新（Tauri updater 插件） =====
const updateState = ref<'idle' | 'checking' | 'available' | 'latest' | 'downloading' | 'error'>('idle')
const updateMessage = ref('')
const updateVersion = ref('')
const downloadProgress = ref(0)

async function checkUpdate() {
  updateState.value = 'checking'
  updateMessage.value = '正在检查更新...'
  try {
    const { check } = await import('@tauri-apps/plugin-updater')
    const update = await check()
    if (!update) {
      updateState.value = 'latest'
      updateMessage.value = '当前已是最新版本'
      return
    }
    updateState.value = 'available'
    updateVersion.value = update.version
    updateMessage.value = `发现新版本 ${update.version}`
  } catch (e) {
    updateState.value = 'error'
    // 常见于未配置 updater 端点/公钥，或当前非打包环境（tauri dev）
    updateMessage.value = `检查更新失败: ${e}`
  }
}

async function installUpdate() {
  updateState.value = 'downloading'
  downloadProgress.value = 0
  updateMessage.value = '正在下载更新...'
  try {
    const { check } = await import('@tauri-apps/plugin-updater')
    const update = await check()
    if (!update) {
      updateState.value = 'latest'
      updateMessage.value = '当前已是最新版本'
      return
    }
    let downloaded = 0
    let contentLength = 0
    await update.downloadAndInstall((event) => {
      switch (event.event) {
        case 'Started':
          contentLength = event.data.contentLength ?? 0
          break
        case 'Progress':
          downloaded += event.data.chunkLength
          downloadProgress.value = contentLength > 0
            ? Math.min(100, Math.round((downloaded / contentLength) * 100))
            : 0
          break
        case 'Finished':
          downloadProgress.value = 100
          updateMessage.value = '下载完成，即将重启安装...'
          break
      }
    })
    const { relaunch } = await import('@tauri-apps/plugin-process')
    await relaunch()
  } catch (e) {
    updateState.value = 'error'
    updateMessage.value = `更新失败: ${e}`
  }
}
</script>

<template>
  <div class="p-6 space-y-6 animate-fade-in max-w-[760px]">
    <div>
      <h1 class="text-xl font-semibold tracking-tight">{{ t('settings.title') }}</h1>
      <p class="text-sm text-t-secondary mt-0.5">自定义 RescueForge 的外观和行为</p>
    </div>

    <section class="card divide-y divide-app-border">
      <!-- 主题（三选） -->
      <div class="p-4">
        <p class="text-sm font-medium mb-3">{{ t('settings.theme') }}</p>
        <div class="grid grid-cols-3 gap-3">
          <button
            v-for="opt in themeOptions"
            :key="opt.key"
            @click="settingsStore.setTheme(opt.key)"
            :class="[
              'flex flex-col items-start gap-2 p-3 rounded-lg border text-left transition-all',
              settingsStore.settings.theme === opt.key
                ? 'border-brand-blue ring-1 ring-brand-blue bg-app-card-hover'
                : 'border-app-border hover:border-app-border-light'
            ]"
          >
            <div class="flex items-center justify-between w-full">
              <span
                class="w-6 h-6 rounded-md border border-app-border"
                :style="{ backgroundColor: opt.swatch }"
              />
              <component :is="opt.icon" class="w-4 h-4" :class="settingsStore.settings.theme === opt.key ? 'text-brand-blue' : 'text-t-muted'" />
            </div>
            <div>
              <p class="text-sm font-medium">{{ opt.label }}</p>
              <p class="text-2xs text-t-muted">{{ opt.desc }}</p>
            </div>
          </button>
        </div>
      </div>

      <!-- 语言：目前仅支持简体中文，移除英文选项 -->
      <div class="flex items-center gap-3 p-4">
        <div class="w-8 h-8 rounded-lg bg-brand-green/10 flex items-center justify-center">
          <Languages class="w-4 h-4 text-brand-green" />
        </div>
        <div>
          <p class="text-sm font-medium">{{ t('settings.language') }}</p>
          <p class="text-2xs text-t-muted">简体中文</p>
        </div>
      </div>

      <!-- 默认恢复路径 -->
      <div class="p-4">
        <div class="flex items-center justify-between">
          <div class="flex items-center gap-3">
            <div class="w-8 h-8 rounded-lg bg-brand-amber/10 flex items-center justify-center">
              <FolderOpen class="w-4 h-4 text-brand-amber" />
            </div>
            <div>
              <p class="text-sm font-medium">{{ t('settings.defaultPath') }}</p>
              <p class="text-2xs text-t-muted">恢复文件的默认保存位置</p>
            </div>
          </div>
          <button @click="browsePath" class="btn-secondary text-xs h-7">
            {{ t('settings.browse') }}
          </button>
        </div>
        <p v-if="settingsStore.settings.default_recovery_path" class="mt-2 ml-11 text-xs font-mono text-t-secondary break-all">
          {{ settingsStore.settings.default_recovery_path }}
        </p>
      </div>

      <!-- 自动更新 -->
      <div class="flex items-center justify-between p-4">
        <div>
          <p class="text-sm font-medium">{{ t('settings.autoUpdate') }}</p>
          <p class="text-2xs text-t-muted">启动时检查新版本</p>
        </div>
        <button
          @click="settingsStore.settings.auto_update = !settingsStore.settings.auto_update"
          :class="[
            'relative w-10 h-5 rounded-full transition-colors duration-200',
            settingsStore.settings.auto_update ? 'bg-brand-blue' : 'bg-app-input border border-app-border'
          ]"
        >
          <span
            :class="[
              'absolute top-0.5 w-4 h-4 rounded-full bg-white transition-transform duration-200 shadow-sm',
              settingsStore.settings.auto_update ? 'translate-x-5' : 'translate-x-0.5'
            ]"
          />
        </button>
      </div>

      <!-- 显示隐藏文件 -->
      <div class="flex items-center justify-between p-4">
        <div>
          <p class="text-sm font-medium">扫描时包含隐藏文件</p>
          <p class="text-2xs text-t-muted">深度扫描将同时索引隐藏/系统文件</p>
        </div>
        <button
          @click="settingsStore.settings.show_hidden_files = !settingsStore.settings.show_hidden_files"
          :class="[
            'relative w-10 h-5 rounded-full transition-colors duration-200',
            settingsStore.settings.show_hidden_files ? 'bg-brand-blue' : 'bg-app-input border border-app-border'
          ]"
        >
          <span
            :class="[
              'absolute top-0.5 w-4 h-4 rounded-full bg-white transition-transform duration-200 shadow-sm',
              settingsStore.settings.show_hidden_files ? 'translate-x-5' : 'translate-x-0.5'
            ]"
          />
        </button>
      </div>
    </section>

    <!-- 关于 + 检查更新 -->
    <section class="card p-4">
      <div class="flex items-center justify-between gap-3">
        <div class="flex items-center gap-3">
          <div class="w-8 h-8 rounded-lg bg-brand-cyan/10 flex items-center justify-center">
            <Info class="w-4 h-4 text-brand-cyan" />
          </div>
          <div>
            <p class="text-sm font-medium">{{ t('settings.about') }}</p>
            <p class="text-2xs text-t-muted">{{ t('settings.version') }} {{ appVersion }} · Tauri 2.x + Vue 3</p>
          </div>
        </div>
        <button
          @click="checkUpdate"
          class="btn-secondary text-xs h-7 flex-shrink-0"
          :disabled="updateState === 'checking' || updateState === 'downloading'"
        >
          <Loader v-if="updateState === 'checking'" class="w-3.5 h-3.5 animate-spin" />
          {{ updateState === 'checking' ? '检查中...' : t('settings.checkUpdate') }}
        </button>
      </div>

      <!-- 更新状态 -->
      <div v-if="updateState !== 'idle'" class="mt-3 pt-3 border-t border-app-border">
        <p
          class="text-xs"
          :class="{
            'text-t-secondary': updateState === 'checking' || updateState === 'downloading',
            'text-brand-green': updateState === 'latest' || updateState === 'available',
            'text-brand-red': updateState === 'error'
          }"
        >
          {{ updateMessage }}
        </p>

        <!-- 下载进度条 -->
        <div v-if="updateState === 'downloading'" class="mt-2 progress-track">
          <div class="progress-fill-blue" :style="{ width: `${downloadProgress}%` }" />
        </div>

        <!-- 发现新版本：提供安装按钮 -->
        <button
          v-if="updateState === 'available'"
          @click="installUpdate"
          class="btn-primary text-xs h-7 mt-2"
        >
          <Download class="w-3.5 h-3.5" />
          下载并安装 v{{ updateVersion }}
        </button>
      </div>
    </section>

    <!-- 保存 -->
    <button @click="save" class="btn-primary w-full">
      <Check v-if="saved" class="w-4 h-4" />
      {{ saved ? t('settings.saved') : t('settings.save') }}
    </button>
  </div>
</template>
