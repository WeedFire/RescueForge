import { createApp } from 'vue'
import { createPinia } from 'pinia'
import App from './App.vue'
import router from './router'
import { i18n } from './i18n'
import './assets/styles/index.css'

// ===== 前端运行时诊断 =====
// 打包后（tauri://localhost 自定义协议）若出现白屏/样式丢失/菜单不可见，
// 前端错误不可见于终端，故统一捕获并写入后端日志：
//   %APPDATA%\RescueForge\rescueforge.log
async function logToBackend(level: string, msg: string) {
  try {
    const { writeAppLog } = await import('@/api/tauri')
    await writeAppLog(level, `[UI] ${msg}`)
  } catch {
    /* 非 Tauri 环境下忽略 */
  }
}

// 捕获阶段监听：资源加载失败（error 事件不冒泡，必须用 capture）
window.addEventListener(
  'error',
  (e) => {
    const t = e.target as HTMLElement | null
    if (t && (t.tagName === 'LINK' || t.tagName === 'SCRIPT')) {
      const url = (t as HTMLLinkElement).href || (t as HTMLScriptElement).src || ''
      void logToBackend('ERROR', `资源加载失败 <${t.tagName}> ${url}`)
      return
    }
    void logToBackend('ERROR', `JS错误 ${e.message} @ ${e.filename}:${e.lineno}:${e.colno}`)
  },
  true,
)

window.addEventListener('unhandledrejection', (e) => {
  void logToBackend('ERROR', `Promise未处理 ${e.reason}`)
})

// i18n 实例统一由 ./i18n 导出（此前 main.ts 与 i18n/index.ts 各建一个，
// 属重复实例，在打包产物中易造成翻译状态不一致，已合并为单一实例）。
const app = createApp(App)
const pinia = createPinia()

app.use(pinia)
app.use(router)
app.use(i18n)

app.mount('#app')

// 样式自检：挂载后校验关键样式是否真正生效。
// 若 CSS 未加载（路径/CSP/协议问题），aside 宽度会是 auto 或 0px 而非 224px。
setTimeout(() => {
  const aside = document.querySelector('aside')
  const width = aside ? getComputedStyle(aside).width : 'NO_ASIDE_ELEMENT'
  const bg = aside ? getComputedStyle(aside).backgroundColor : '-'
  void logToBackend('INFO', `样式自检 aside宽度=${width} 背景=${bg}（正常应为 224px）`)
}, 2000)
