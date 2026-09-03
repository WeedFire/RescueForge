// 使用 hash 路由：提权重启后 exe 直接加载内嵌 dist（tauri.localhost 自定义协议），
// WebHistory 的 pushState 路径导航在该协议下无 SPA 回退，会导致点击导航无反应；
// hash 模式仅改变 # 后的片段，任何加载环境下均可正常导航。
import { createRouter, createWebHashHistory } from 'vue-router'

const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    {
      path: '/',
      name: 'dashboard',
      component: () => import('@/views/Dashboard.vue'),
      meta: { title: '仪表盘', transition: 'fade', icon: 'LayoutDashboard' },
    },
    {
      path: '/scanning',
      name: 'scanning',
      component: () => import('@/views/ScanningView.vue'),
      meta: { title: '扫描进行时', transition: 'slide', icon: 'Search' },
    },
    {
      path: '/results',
      name: 'results',
      component: () => import('@/views/ResultsView.vue'),
      meta: { title: '扫描结果', transition: 'slide', icon: 'FolderOpen' },
    },
    {
      path: '/partition-repair',
      name: 'partition-repair',
      component: () => import('@/views/PartitionRepair.vue'),
      meta: { title: '分区修复', transition: 'fade', icon: 'Wrench' },
    },
    {
      path: '/settings',
      name: 'settings',
      component: () => import('@/views/Settings.vue'),
      meta: { title: '设置', transition: 'fade', icon: 'Settings' },
    },
  ],
})

router.beforeEach((to, _from, next) => {
  document.title = `${to.meta.title as string} - RescueForge`
  next()
})

export default router
