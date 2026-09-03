import { defineStore } from 'pinia'
import { ref } from 'vue'

export const useUiStore = defineStore('ui', () => {
  const showSmartModal = ref(false)
  const showConfirmDialog = ref(false)
  const confirmDialogConfig = ref({
    title: '',
    message: '',
    confirmText: '确认',
    cancelText: '取消',
    danger: false,
  })
  const toastMessage = ref('')
  const toastType = ref<'success' | 'error' | 'warning' | 'info'>('info')
  const toastVisible = ref(false)

  function openSmartModal() {
    showSmartModal.value = true
  }

  function closeSmartModal() {
    showSmartModal.value = false
  }

  function showConfirm(config: {
    title: string
    message: string
    confirmText?: string
    cancelText?: string
    danger?: boolean
  }) {
    confirmDialogConfig.value = {
      title: config.title,
      message: config.message,
      confirmText: config.confirmText || '确认',
      cancelText: config.cancelText || '取消',
      danger: config.danger || false,
    }
    showConfirmDialog.value = true
  }

  function hideConfirm() {
    showConfirmDialog.value = false
  }

  function showToast(message: string, type: 'success' | 'error' | 'warning' | 'info' = 'info') {
    toastMessage.value = message
    toastType.value = type
    toastVisible.value = true
    setTimeout(() => {
      toastVisible.value = false
    }, 3000)
  }

  return {
    showSmartModal,
    showConfirmDialog,
    confirmDialogConfig,
    toastMessage,
    toastType,
    toastVisible,
    openSmartModal,
    closeSmartModal,
    showConfirm,
    hideConfirm,
    showToast,
  }
})
