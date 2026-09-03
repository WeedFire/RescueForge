<script setup lang="ts">
import { onMounted } from 'vue'
import { RouterView } from 'vue-router'
import AppShell from '@/components/layout/AppShell.vue'
import { useAutotest } from '@/composables/useAutotest'

const { state: autotest, maybeRun } = useAutotest()
onMounted(() => {
  maybeRun()
})
</script>

<template>
  <AppShell>
    <RouterView v-slot="{ Component, route }">
      <transition
        :name="(route.meta.transition as string) || 'fade'"
        mode="out-in"
      >
        <component :is="Component" :key="route.path" />
      </transition>
    </RouterView>
  </AppShell>

  <!-- 自动冒烟测试结果面板（仅 RF_AUTOTEST=1 时显示） -->
  <div
    v-if="autotest.enabled"
    class="fixed top-4 right-4 z-50 w-96 max-h-[80vh] overflow-auto rounded-lg border p-3 text-xs shadow-xl bg-white/95 dark:bg-gray-900/95"
    :class="autotest.finished ? (autotest.passed ? 'border-green-500' : 'border-red-500') : 'border-blue-400'"
  >
    <div class="mb-2 flex items-center justify-between font-bold">
      <span>自动冒烟测试</span>
      <span v-if="autotest.running" class="text-blue-500">运行中...</span>
      <span v-else-if="autotest.passed" class="text-green-500">通过</span>
      <span v-else-if="autotest.finished" class="text-red-500">失败</span>
    </div>
    <div v-if="autotest.recoverResult" class="mb-2">
      发现 {{ autotest.foundFiles.length }} 个文件，恢复成功 {{ autotest.recoverResult.success }}，失败 {{ autotest.recoverResult.failed }}
      <div class="text-gray-500">输出: {{ autotest.outputPath }}</div>
    </div>
    <div v-for="(line, i) in autotest.logs" :key="i" class="whitespace-pre-wrap break-all py-0.5 font-mono text-[10px] leading-tight text-gray-700 dark:text-gray-300">
      {{ line }}
    </div>
  </div>
</template>

<style>
.fade-enter-active,
.fade-leave-active {
  transition: opacity 0.2s ease;
}
.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}
.slide-enter-active,
.slide-leave-active {
  transition: all 0.25s ease;
}
.slide-enter-from {
  opacity: 0;
  transform: translateX(16px);
}
.slide-leave-to {
  opacity: 0;
  transform: translateX(-16px);
}
</style>
