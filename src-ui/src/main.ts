import { createApp } from 'vue'
import { createPinia } from 'pinia'
import App from './App.vue'
import router from './router'
import { i18n } from './i18n'
import './assets/styles/index.css'

// i18n 实例统一由 ./i18n 导出（此前 main.ts 与 i18n/index.ts 各建一个，
// 属重复实例，在打包产物中易造成翻译状态不一致，已合并为单一实例）。
const app = createApp(App)
const pinia = createPinia()

app.use(pinia)
app.use(router)
app.use(i18n)

app.mount('#app')
