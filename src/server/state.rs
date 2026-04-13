use crate::error::Result;
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::config::{DEFAULT_CONCURRENT_DOWNLOADS, DEFAULT_RETRY_COUNT, HTTP_TIMEOUT_SECONDS};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub download_dir: String,
    pub temp_dir: String,
    pub concurrent: usize,
    pub retry: usize,
    pub ffmpeg_path: String,
    pub timeout: u64,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            download_dir: "./output".to_string(),
            temp_dir: "./temp".to_string(),
            concurrent: DEFAULT_CONCURRENT_DOWNLOADS,
            retry: DEFAULT_RETRY_COUNT,
            ffmpeg_path: String::new(),
            timeout: HTTP_TIMEOUT_SECONDS,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,
    Downloading,
    Merging,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskInfo {
    pub id: String,
    pub name: String,
    pub url: String,
    pub status: TaskStatus,
    pub progress: f64,
    pub created_at: DateTime<Local>,
    pub updated_at: DateTime<Local>,
    pub error: Option<String>,
    pub output_file: Option<String>,
    pub file_size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadRequest {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub output_dir: Option<String>,
}

#[derive(Clone)]
pub struct AppState {
    pub tasks: Arc<RwLock<HashMap<String, TaskInfo>>>,
    pub settings: Arc<RwLock<AppSettings>>,
    pub settings_file: PathBuf,
    pub data_file: PathBuf,
}

impl AppState {
    pub fn new(data_file: PathBuf, settings_file: PathBuf) -> Self {
        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            settings: Arc::new(RwLock::new(AppSettings::default())),
            settings_file,
            data_file,
        }
    }

    pub async fn load(&self) -> Result<()> {
        if self.data_file.exists() {
            let content = tokio::fs::read_to_string(&self.data_file).await?;
            let tasks: HashMap<String, TaskInfo> = serde_json::from_str(&content)?;
            let len = {
                let mut lock = self.tasks.write().await;
                *lock = tasks;
                lock.len()
            };
            log::info!("已加载 {len} 个历史任务");
        }

        if self.settings_file.exists() {
            let content = tokio::fs::read_to_string(&self.settings_file).await?;
            let settings: AppSettings = serde_json::from_str(&content)?;
            *self.settings.write().await = settings;
            log::info!("✅ 已加载设置: {:?}", self.settings.read().await);
        } else {
            log::info!(
                "📝 设置文件不存在，使用默认设置: {:?}",
                self.settings.read().await
            );
        }

        Ok(())
    }

    pub async fn save(&self) -> Result<()> {
        let tasks_content = serde_json::to_string_pretty(&*self.tasks.read().await)?;

        if let Some(parent) = self.data_file.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        tokio::fs::write(&self.data_file, tasks_content).await?;
        Ok(())
    }

    pub async fn save_settings(&self) -> Result<()> {
        let settings_content = serde_json::to_string_pretty(&*self.settings.read().await)?;

        if let Some(parent) = self.settings_file.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        tokio::fs::write(&self.settings_file, settings_content).await?;
        log::info!("设置已保存");
        Ok(())
    }

    pub async fn get_settings(&self) -> AppSettings {
        self.settings.read().await.clone()
    }

    pub async fn update_settings(&self, new_settings: AppSettings) -> Result<()> {
        *self.settings.write().await = new_settings;
        self.save_settings().await?;
        Ok(())
    }

    pub async fn add_task(&self, request: DownloadRequest) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        let now = Local::now();

        let task = TaskInfo {
            id: id.clone(),
            name: request.name,
            url: request.url,
            status: TaskStatus::Pending,
            progress: 0.0,
            created_at: now,
            updated_at: now,
            error: None,
            output_file: None,
            file_size: None,
        };

        {
            let mut tasks = self.tasks.write().await;
            tasks.insert(id.clone(), task);
        }

        self.save().await?;
        Ok(id)
    }

    pub async fn update_task_status(
        &self,
        id: &str,
        status: TaskStatus,
        error: Option<String>,
    ) -> Result<()> {
        {
            let mut tasks = self.tasks.write().await;
            if let Some(task) = tasks.get_mut(id) {
                task.status = status;
                task.error = error;
                task.updated_at = Local::now();
            }
        }
        self.save().await?;
        Ok(())
    }

    pub async fn update_task_progress(&self, id: &str, progress: f64) -> Result<()> {
        {
            let mut tasks = self.tasks.write().await;
            if let Some(task) = tasks.get_mut(id) {
                task.progress = progress;
                task.updated_at = Local::now();
            }
        }
        // 进度更新不每次都保存，减少IO
        Ok(())
    }

    pub async fn update_task_output(
        &self,
        id: &str,
        output_file: String,
        file_size: u64,
    ) -> Result<()> {
        {
            let mut tasks = self.tasks.write().await;
            if let Some(task) = tasks.get_mut(id) {
                task.output_file = Some(output_file);
                task.file_size = Some(file_size);
                task.updated_at = Local::now();
            }
        }
        self.save().await?;
        Ok(())
    }

    pub async fn get_task(&self, id: &str) -> Option<TaskInfo> {
        let tasks = self.tasks.read().await;
        tasks.get(id).cloned()
    }

    pub async fn get_all_tasks(&self) -> Vec<TaskInfo> {
        let mut result: Vec<TaskInfo> = self.tasks.read().await.values().cloned().collect();
        result.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        result
    }

    pub async fn get_tasks_by_status(&self, status: TaskStatus) -> Vec<TaskInfo> {
        let mut result: Vec<TaskInfo> = self
            .tasks
            .read()
            .await
            .values()
            .filter(|t| t.status == status)
            .cloned()
            .collect();
        result.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        result
    }

    pub async fn delete_task(&self, id: &str) -> Result<bool> {
        let removed = {
            let mut tasks = self.tasks.write().await;
            tasks.remove(id).is_some()
        };
        if removed {
            self.save().await?;
        }
        Ok(removed)
    }

    pub async fn search_tasks(&self, query: &str) -> Vec<TaskInfo> {
        let result: Vec<TaskInfo> = self
            .tasks
            .read()
            .await
            .iter()
            .filter(|(_, task)| task.name.to_lowercase().contains(&query.to_lowercase()))
            .map(|(_, task)| task.clone())
            .collect();
        let mut result = result;
        result.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        result
    }

    pub async fn get_stats(&self) -> TaskStats {
        let tasks = self.tasks.read().await;
        TaskStats {
            total: tasks.len(),
            pending: tasks
                .values()
                .filter(|t| t.status == TaskStatus::Pending)
                .count(),
            downloading: tasks
                .values()
                .filter(|t| t.status == TaskStatus::Downloading)
                .count(),
            completed: tasks
                .values()
                .filter(|t| t.status == TaskStatus::Completed)
                .count(),
            failed: tasks
                .values()
                .filter(|t| t.status == TaskStatus::Failed)
                .count(),
            total_size: tasks.values().filter_map(|t| t.file_size).sum(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskStats {
    pub total: usize,
    pub pending: usize,
    pub downloading: usize,
    pub completed: usize,
    pub failed: usize,
    pub total_size: u64,
}
