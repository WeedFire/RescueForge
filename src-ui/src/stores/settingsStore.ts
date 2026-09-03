import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import type { UserSettings, ThemeMode } from '@/types'

export const useSettingsStore = defineStore('settings', () => {
  const settings = ref<UserSettings>({
    theme: 'eye',
    language: 'zh-CN',
    default_recovery_path: '',
    auto_update: true,
    show_hidden_files: false,
  })

  const isDark = computed(() => settings.value.theme === 'dark')

  // 主题轮换：护眼 → 深色 → 浅色 → 护眼
  function toggleTheme() {
    const order: ThemeMode[] = ['eye', 'dark', 'light']
    const i = order.indexOf(settings.value.theme)
    settings.value.theme = order[(i + 1) % order.length]
    applyTheme()
  }

  function setTheme(theme: ThemeMode) {
    settings.value.theme = theme
    applyTheme()
  }

  function applyTheme() {
    const root = document.documentElement
    root.dataset.theme = settings.value.theme
    root.classList.toggle('dark', settings.value.theme === 'dark')
  }

  function setLanguage(lang: string) {
    settings.value.language = lang
  }

  async function loadSettings() {
    try {
      const { getSettings } = await import('@/api/tauri')
      const stored = await getSettings()
      if (stored && typeof stored === 'object') {
        settings.value = { ...settings.value, ...stored }
        // 兼容旧配置：非法主题回退护眼色
        if (!['eye', 'dark', 'light'].includes(settings.value.theme)) {
          settings.value.theme = 'eye'
        }
      }
    } catch {
      // Use defaults
    }
    applyTheme()
  }

  async function saveSettings() {
    try {
      const { updateSettings } = await import('@/api/tauri')
      await updateSettings(settings.value as unknown as Record<string, unknown>)
    } catch (e) {
      console.error('Failed to save settings:', e)
    }
  }

  return {
    settings,
    isDark,
    toggleTheme,
    setTheme,
    applyTheme,
    setLanguage,
    loadSettings,
    saveSettings,
  }
})
