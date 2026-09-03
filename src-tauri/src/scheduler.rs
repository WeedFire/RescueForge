use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

/// Simple cancellation token using AtomicBool
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            paused: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    /// 协作式检查点：若已暂停则阻塞等待恢复；期间被取消或未取消时恢复返回 true，已取消返回 false
    pub fn wait_while_paused(&self) -> bool {
        while self.is_paused() {
            if self.is_cancelled() {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        !self.is_cancelled()
    }
}

/// 任务优先级
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum JobPriority {
    Low = 0,
    Normal = 1,
    High = 2,
    Critical = 3,
}

/// 扫描任务
#[derive(Debug, Clone)]
pub struct ScanJob {
    pub id: String,
    pub disk_index: u32,
    pub scan_mode: String,
    pub target_paths: Option<Vec<String>>,
    pub signature_ids: Option<Vec<String>>,
    pub priority: JobPriority,
    pub cancel_token: CancellationToken,
}

/// 任务调度器 — 延迟启动模式：
/// `new()` 只创建 channel 和状态，不 spawn 任何 Tokio 任务。
/// 必须在 Tokio 运行时就绪后调用 `start()` 启动后台消费循环。
/// `start()` 使用 `std::thread::spawn` + 独立 Tokio runtime 避免 reactor 上下文依赖。
pub struct TaskScheduler {
    job_tx: mpsc::UnboundedSender<ScanJob>,
    /// 未启动时为 Some(rx)，启动后 take 为 None
    pending_rx: std::sync::Mutex<Option<mpsc::UnboundedReceiver<ScanJob>>>,
    active_jobs: Arc<Mutex<Vec<ScanJob>>>,
}

impl TaskScheduler {
    /// 创建调度器（不启动后台任务）
    pub fn new() -> Self {
        let (job_tx, job_rx) = mpsc::unbounded_channel::<ScanJob>();
        let active_jobs = Arc::new(Mutex::new(Vec::new()));

        Self {
            job_tx,
            pending_rx: std::sync::Mutex::new(Some(job_rx)),
            active_jobs,
        }
    }

    /// 在任意上下文中调用（不需要 Tokio runtime），启动后台任务消费循环。
    /// 内部用 `std::thread::spawn` + 独立 Tokio runtime。
    pub fn start(&self) {
        let maybe_rx = self.pending_rx.lock().unwrap().take();
        if let Some(mut job_rx) = maybe_rx {
            let jobs_ref = self.active_jobs.clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().expect("Failed to create scheduler runtime");
                rt.block_on(async move {
                    while let Some(job) = job_rx.recv().await {
                        let mut jobs = jobs_ref.lock().await;
                        jobs.push(job);
                    }
                });
            });
        }
    }

    /// 提交新任务
    pub fn submit(&self, job: ScanJob) {
        self.job_tx.send(job).ok();
    }

    /// 获取活跃任务数
    pub async fn active_count(&self) -> usize {
        self.active_jobs.lock().await.len()
    }
}

/// 内存保护：当系统内存低于阈值时降低并行度
pub struct MemoryGuard {
    warning_threshold_mb: u64,
    critical_threshold_mb: u64,
}

impl MemoryGuard {
    pub fn new() -> Self {
        Self {
            warning_threshold_mb: 1024,
            critical_threshold_mb: 512,
        }
    }

    /// 检查内存状态，返回建议的并行度因子（0.0-1.0）
    pub fn check_memory(&self) -> f32 {
        // TODO: 实现真实的内存检测逻辑
        1.0
    }
}
