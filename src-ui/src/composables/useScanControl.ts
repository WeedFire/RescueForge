import { useScanStore } from '@/stores/scanStore'
import { useRouter } from 'vue-router'

export function useScanControl() {
  const scanStore = useScanStore()
  const router = useRouter()

  async function startScan(diskIndex: number, mode: 'quick' | 'deep' | 'raw') {
    await scanStore.startScan(diskIndex, mode)
    router.push('/scanning')
  }

  async function pause() {
    await scanStore.pauseScan()
  }

  async function resume() {
    await scanStore.resumeScan()
  }

  async function cancel() {
    await scanStore.cancelScan()
    router.push('/')
  }

  return {
    startScan,
    pause,
    resume,
    cancel,
  }
}
