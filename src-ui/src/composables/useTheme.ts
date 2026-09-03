import { watch, computed } from 'vue'
import { useSettingsStore } from '@/stores/settingsStore'

export function useTheme() {
  const settingsStore = useSettingsStore()

  // 跟随主题变化同步到 document（data-theme 驱动 CSS 变量）
  watch(
    () => settingsStore.settings.theme,
    () => settingsStore.applyTheme(),
    { immediate: true }
  )

  return {
    isDark: computed(() => settingsStore.isDark),
    theme: computed(() => settingsStore.settings.theme),
    toggle: () => settingsStore.toggleTheme(),
    setTheme: (t: 'eye' | 'dark' | 'light') => settingsStore.setTheme(t),
    setDark: () => settingsStore.setTheme('dark'),
    setLight: () => settingsStore.setTheme('light'),
  }
}
