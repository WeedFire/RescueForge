<script setup lang="ts">
import { useRoute, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import {
  LayoutDashboard, Search, FolderOpen, Wrench, Settings, HardDrive
} from 'lucide-vue-next'

const route = useRoute()
const router = useRouter()
const { t } = useI18n()

const navItems = [
  { path: '/', icon: LayoutDashboard, label: 'dashboard', color: 'text-brand-blue' },
  { path: '/scanning', icon: Search, label: 'scanning', color: 'text-brand-cyan' },
  { path: '/results', icon: FolderOpen, label: 'results', color: 'text-brand-green' },
  { path: '/partition-repair', icon: Wrench, label: 'partitionRepair', color: 'text-brand-amber' },
  { path: '/settings', icon: Settings, label: 'settings', color: 'text-t-muted' },
]

function isActive(path: string) {
  if (path === '/') return route.path === '/'
  return route.path.startsWith(path)
}
</script>

<template>
  <aside class="flex flex-col w-[224px] flex-shrink-0 bg-app-sidebar border-r border-app-border">
    <!-- Logo -->
    <div class="flex items-center gap-3 h-14 px-4 border-b border-app-border">
      <div class="w-8 h-8 rounded-lg bg-gradient-to-br from-brand-blue to-brand-cyan flex items-center justify-center flex-shrink-0">
        <HardDrive class="w-4 h-4 text-white" />
      </div>
      <span class="font-bold text-sm tracking-tight whitespace-nowrap">RescueForge</span>
    </div>

    <!-- Nav Section -->
    <!-- 注意：文字一律直接渲染，不要用 <transition> 包裹。
         此前用 fade transition 时，打包（production）下时序与 dev 不同，
         .fade-enter-from 的 opacity:0 可能未及时移除，导致菜单只显示图标、文字不可见。 -->
    <nav class="flex-1 py-4 px-3 space-y-1">
      <p class="px-2 pb-2 text-3xs font-semibold text-t-muted uppercase tracking-widest">导航</p>
      <button
        v-for="item in navItems"
        :key="item.path"
        @click="router.push(item.path)"
        :class="[
          'w-full flex items-center gap-3 h-9 px-3 rounded-lg transition-all duration-150',
          isActive(item.path)
            ? 'bg-brand-blue/12 text-brand-blue shadow-[inset_0_0_0_1px_rgba(59,130,246,0.15)]'
            : 'text-t-secondary hover:text-t-primary hover:bg-app-card'
        ]"
      >
        <component :is="item.icon" :class="['w-[18px] h-[18px] flex-shrink-0', isActive(item.path) ? item.color : '']" />
        <span class="text-sm truncate">{{ t(`nav.${item.label}`) }}</span>
        <div v-if="isActive(item.path)" class="ml-auto w-1.5 h-1.5 rounded-full bg-brand-blue" />
      </button>
    </nav>
  </aside>
</template>
