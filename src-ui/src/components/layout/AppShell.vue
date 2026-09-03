<script setup lang="ts">
import { ref, onMounted } from 'vue'
import Sidebar from './Sidebar.vue'
import StatusBar from './StatusBar.vue'
import { useTauriEvent } from '@/composables/useTauriEvent'
import { useSettingsStore } from '@/stores/settingsStore'
import { getPrivilegeStatus, restartElevated } from '@/api/tauri'
import { ShieldAlert, X } from 'lucide-vue-next'

const settingsStore = useSettingsStore()
useTauriEvent()

// 管理员权限状态：深度恢复永久删除文件、分区修复、磁盘镜像需要
const elevated = ref<boolean | null>(null)
const bannerDismissed = ref(false)
const restarting = ref(false)

async function refreshPrivilege() {
  try {
    const st = await getPrivilegeStatus()
    elevated.value = st.elevated
  } catch {
    elevated.value = null
  }
}

async function restartAsAdmin() {
  restarting.value = true
  try {
    await restartElevated()
  } catch (e) {
    alert(`${e}`)
    restarting.value = false
  }
}

onMounted(() => {
  settingsStore.loadSettings()
  refreshPrivilege()
})
</script>

<template>
  <div class="flex h-screen bg-app-bg text-t-primary overflow-hidden">
    <Sidebar />
    <div class="flex flex-col flex-1 min-w-0">
      <!-- 管理员提权横幅 -->
      <div
        v-if="elevated === false && !bannerDismissed"
        class="flex items-center gap-3 px-4 py-2 bg-brand-amber/10 border-b border-brand-amber/25 text-sm"
      >
        <ShieldAlert class="w-4 h-4 text-brand-amber flex-shrink-0" />
        <p class="flex-1 text-t-secondary min-w-0 truncate">
          <span class="text-t-primary font-medium">当前非管理员运行</span>
          · 永久删除文件恢复、分区修复、磁盘镜像需要管理员权限
        </p>
        <button
          @click="restartAsAdmin"
          class="btn-primary-sm flex-shrink-0"
          :disabled="restarting"
        >
          {{ restarting ? '正在提权...' : '以管理员身份重启' }}
        </button>
        <button @click="bannerDismissed = true" class="text-t-muted hover:text-t-primary flex-shrink-0">
          <X class="w-3.5 h-3.5" />
        </button>
      </div>

      <main class="flex-1 overflow-y-auto overflow-x-hidden">
        <slot />
      </main>
      <StatusBar />
    </div>
  </div>
</template>
