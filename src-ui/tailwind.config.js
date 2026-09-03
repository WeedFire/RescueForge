/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{vue,js,ts,jsx,tsx}"],
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
        // 基底色：全部走 CSS 变量（RGB 三元组，支持透明度修饰符），支持 护眼/深色/浅色 三套主题（见 index.css）
        'app-bg': 'rgb(var(--app-bg) / <alpha-value>)',
        'app-sidebar': 'rgb(var(--app-sidebar) / <alpha-value>)',
        'app-card': 'rgb(var(--app-card) / <alpha-value>)',
        'app-card-hover': 'rgb(var(--app-card-hover) / <alpha-value>)',
        'app-input': 'rgb(var(--app-input) / <alpha-value>)',
        'app-border': 'rgb(var(--app-border) / <alpha-value>)',
        'app-border-light': 'rgb(var(--app-border-light) / <alpha-value>)',

        // 文字
        't-primary': 'rgb(var(--t-primary) / <alpha-value>)',
        't-secondary': 'rgb(var(--t-secondary) / <alpha-value>)',
        't-muted': 'rgb(var(--t-muted) / <alpha-value>)',

        // 强调色 - 数据恢复专业色（使用 rgb 格式以支持透明度简写 bg-brand-red/12）
        'brand-blue': 'rgb(59 130 246)',
        'brand-cyan': 'rgb(6 182 212)',
        'brand-green': 'rgb(34 197 94)',
        'brand-amber': 'rgb(245 158 11)',
        'brand-red': 'rgb(239 68 68)',
        'brand-purple': 'rgb(139 92 246)',
        'brand-pink': 'rgb(236 72 153)',
      },
      fontFamily: {
        sans: ['Inter', 'SF Pro Display', 'PingFang SC', 'Microsoft YaHei', 'sans-serif'],
        mono: ['JetBrains Mono', 'SF Mono', 'Fira Code', 'Consolas', 'monospace'],
      },
      fontSize: {
        '3xs': ['0.625rem', { lineHeight: '0.875rem' }],
        '2xs': ['0.6875rem', { lineHeight: '1rem' }],
        'xs': ['0.75rem', { lineHeight: '1rem' }],
        'sm': ['0.8125rem', { lineHeight: '1.25rem' }],
        'base': ['0.875rem', { lineHeight: '1.5rem' }],
        'lg': ['1rem', { lineHeight: '1.5rem' }],
        'xl': ['1.125rem', { lineHeight: '1.75rem' }],
        '2xl': ['1.375rem', { lineHeight: '1.875rem' }],
        '3xl': ['1.75rem', { lineHeight: '2.25rem' }],
      },
      borderRadius: {
        'sm': '4px',
        'md': '6px',
        'lg': '8px',
        'xl': '12px',
        '2xl': '16px',
      },
      boxShadow: {
        'card': '0 1px 2px rgba(0,0,0,0.3)',
        'card-hover': '0 4px 16px rgba(0,0,0,0.4), 0 0 0 1px rgba(59,130,246,0.1)',
      },
      animation: {
        'spin-slow': 'spin 2s linear infinite',
        'pulse-dot': 'pulseDot 1.5s ease-in-out infinite',
      },
      keyframes: {
        pulseDot: {
          '0%, 100%': { opacity: '1' },
          '50%': { opacity: '0.4' },
        },
      },
    },
  },
  plugins: [],
}
