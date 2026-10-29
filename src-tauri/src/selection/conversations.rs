use super::*;
use std::collections::BTreeSet;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ConversationView {
    card: Option<RequirementCard>,
    description: String,
    messages: Vec<String>,
    editing: bool,
}

#[derive(Clone, Deserialize, Serialize)]
struct SavedConversation {
    session: SelectionSession,
    view: Option<ConversationView>,
    updated_at_ms: u64,
    #[serde(default)]
    pinned: bool,
    #[serde(default)]
    pin_updated_at_ms: u64,
}

#[derive(Clone, Default, Deserialize, Serialize)]
pub(super) struct History {
    active_id: String,
    sessions: BTreeMap<String, SavedConversation>,
    #[serde(default)]
    deleted_ids: BTreeSet<String>,
    #[serde(skip)]
    path: Option<PathBuf>,
    #[serde(skip)]
    load_error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConversationSummary {
    id: String,
    title: String,
    updated_at_ms: u64,
    pinned: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Workspace {
    session: SelectionSession,
    view: Option<ConversationView>,
    conversations: Vec<ConversationSummary>,
}

#[derive(Serialize)]
pub(crate) struct ConversationIndex {
    view: Option<ConversationView>,
    conversations: Vec<ConversationSummary>,
}

impl History {
    fn record(&mut self, session: &SelectionSession, view: Option<ConversationView>) {
        let (pinned, pin_updated_at_ms) = self
            .sessions
            .get(&session.id)
            .map(|saved| (saved.pinned, saved.pin_updated_at_ms))
            .unwrap_or_default();
        self.sessions.insert(
            session.id.clone(),
            SavedConversation {
                session: session.clone(),
                view,
                updated_at_ms: now_ms(),
                pinned,
                pin_updated_at_ms,
            },
        );
        self.active_id = session.id.clone();
    }

    fn summaries(&self) -> Vec<ConversationSummary> {
        let mut items: Vec<_> = self
            .sessions
            .values()
            .map(|saved| {
                let card = saved
                    .view
                    .as_ref()
                    .and_then(|view| view.card.as_ref())
                    .or_else(|| {
                        saved
                            .session
                            .confirmed
                            .as_ref()
                            .map(|confirmed| &confirmed.card)
                    })
                    .or(saved.session.draft.as_ref());
                let name = card
                    .and_then(|card| card.tasks.first())
                    .map(|task| task.name.as_str())
                    .filter(|name| !name.trim().is_empty() && *name != "待确认任务")
                    .unwrap_or_else(|| {
                        saved
                            .view
                            .as_ref()
                            .and_then(|view| view.messages.first())
                            .map(String::as_str)
                            .filter(|name| !name.trim().is_empty())
                            .unwrap_or(&saved.session.description)
                    });
                ConversationSummary {
                    id: saved.session.id.clone(),
                    title: if name.trim().is_empty() {
                        "新选型".into()
                    } else {
                        name.lines()
                            .next()
                            .unwrap_or(name)
                            .chars()
                            .take(32)
                            .collect()
                    },
                    updated_at_ms: saved.updated_at_ms,
                    pinned: saved.pinned,
                }
            })
            .collect();
        items.sort_by(|a, b| {
            b.pinned.cmp(&a.pinned).then_with(|| {
                b.updated_at_ms
                    .cmp(&a.updated_at_ms)
                    .then_with(|| a.id.cmp(&b.id))
            })
        });
        items
    }

    fn persist(&mut self) -> AppResult<()> {
        if let Some(error) = &self.load_error {
            return Err(validation(error));
        }
        let Some(path) = &self.path else {
            return Ok(());
        };
        let parent = path
            .parent()
            .ok_or_else(|| validation("选型对话目录无效。"))?;
        fs::create_dir_all(parent).map_err(|_| validation("无法创建选型对话目录。"))?;
        let mut lock_options = OpenOptions::new();
        lock_options
            .create(true)
            .truncate(false)
            .read(true)
            .write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            lock_options.mode(0o600);
        }
        let lock = lock_options
            .open(path.with_extension("lock"))
            .map_err(|_| validation("无法锁定选型对话文件。"))?;
        lock.lock()
            .map_err(|_| validation("无法锁定选型对话文件。"))?;
        // 多个本地窗口写同一历史文件时，不能覆盖另一窗口的新对话。
        match fs::read(path) {
            Ok(bytes) => {
                let disk: History = serde_json::from_slice(&bytes)
                    .map_err(|_| validation("选型对话文件无法读取，原文件已保留。"))?;
                self.deleted_ids.extend(disk.deleted_ids);
                for (id, saved) in disk.sessions {
                    if let Some(local) = self.sessions.get_mut(&id) {
                        let pin = if saved.pin_updated_at_ms > local.pin_updated_at_ms {
                            (saved.pinned, saved.pin_updated_at_ms)
                        } else {
                            (local.pinned, local.pin_updated_at_ms)
                        };
                        if saved.updated_at_ms > local.updated_at_ms {
                            *local = saved;
                        }
                        (local.pinned, local.pin_updated_at_ms) = pin;
                    } else {
                        self.sessions.insert(id, saved);
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(validation("无法读取本机选型对话，原文件已保留。")),
        }
        // 删除标记只保存 ID，避免旧窗口的自动保存把已删对话重新写回来。
        self.sessions.retain(|id, _| !self.deleted_ids.contains(id));
        if self.deleted_ids.contains(&self.active_id) {
            return Err(validation(
                "当前选型对话已在另一窗口删除，请切换或新建选型。",
            ));
        }
        let bytes = serde_json::to_vec(self).map_err(|_| validation("无法保存选型对话。"))?;
        let temp = path.with_file_name(format!("selection-history-{}.tmp", Uuid::new_v4()));
        let result = (|| {
            let mut options = OpenOptions::new();
            options.create_new(true).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options
                .open(&temp)
                .map_err(|_| validation("无法写入选型对话。"))?;
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|_| validation("选型对话保存未完成。"))?;
            fs::rename(&temp, path).map_err(|_| validation("无法更新选型对话文件。"))
        })();
        if result.is_err() {
            let _ = fs::remove_file(temp);
        }
        result
    }
}

impl SelectionState {
    pub(crate) fn load(path: PathBuf) -> AppResult<Self> {
        let mut history: History = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|_| History {
                load_error: Some("选型对话文件无法读取，原文件已保留，暂不写入历史。".into()),
                ..Default::default()
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => History::default(),
            Err(_) => History {
                load_error: Some("无法读取本机选型对话，原文件已保留，暂不写入历史。".into()),
                ..Default::default()
            },
        };
        history.path = Some(path);
        for saved in history.sessions.values_mut() {
            let session = &mut saved.session;
            session.running = false;
            session.progress = None;
            session.storage_error.clear();
            if session.metrics.completed_calls == 0
                && let Some(invocation) = session.invocation.clone()
            {
                session.metrics.record(invocation);
            }
            for card in session
                .draft
                .iter_mut()
                .chain(
                    session
                        .confirmed
                        .iter_mut()
                        .map(|confirmed| &mut confirmed.card),
                )
                .chain(saved.view.iter_mut().filter_map(|view| view.card.as_mut()))
            {
                migrate_legacy_io(card);
            }
            research::invalidate_outdated_research(session);
            for run in session.task_runs.values_mut() {
                if let Some(report) = run
                    .research
                    .as_mut()
                    .and_then(|result| result.report.as_mut())
                {
                    report.discard_legacy_ratings();
                    report.assign_recommendations();
                }
                if let Some(result) = &mut run.research {
                    result.needs_ratings = result
                        .report
                        .as_ref()
                        .is_some_and(|report| report.needs_ratings());
                    if result.needs_ratings && result.rating_version < 1 {
                        result.rating_attempted = false;
                    }
                }
                if let Some(result) = &mut run.research
                    && result.status == "running"
                {
                    result.status = "failed".into();
                    result.error = "上次调研已中断，可以重新调研。".into();
                }
            }
        }
        let current = history
            .sessions
            .get(&history.active_id)
            .map(|saved| saved.session.clone())
            .unwrap_or_default();
        Ok(Self(Mutex::new(current), Mutex::new(history)))
    }

    pub(super) fn checkpoint(&self, current: &mut SelectionSession, refresh_view: bool) {
        current.storage_error.clear();
        let result = (|| {
            let mut history = self.1.lock().map_err(|_| lock_error())?;
            let mut next = history.clone();
            let view = if refresh_view {
                None
            } else {
                next.sessions
                    .get(&current.id)
                    .and_then(|saved| saved.view.clone())
            };
            next.record(current, view);
            next.persist()?;
            *history = next;
            Ok::<_, AppError>(())
        })();
        if let Err(error) = result {
            current.storage_error = error.message;
        }
    }
}

fn validate_view(view: &ConversationView) -> AppResult<()> {
    // 编辑中的卡片允许暂时留空；正式确认仍由 validate_card 校验。
    if serde_json::to_vec(view).map_or(true, |bytes| bytes.len() > 1_000_000) {
        return Err(validation("本轮对话草稿过大，请缩短后保存。"));
    }
    Ok(())
}

fn switch_conversation(
    current: &mut SelectionSession,
    history: &mut History,
    view: ConversationView,
    target_id: Option<String>,
) -> AppResult<Workspace> {
    if current.running {
        return Err(validation("当前正在生成，请完成后再切换选型对话。"));
    }
    validate_view(&view)?;
    let mut next = history.clone();
    next.record(current, Some(view));
    let (target, view) = if let Some(id) = target_id {
        let saved = next
            .sessions
            .get(&id)
            .ok_or_else(|| validation("该选型对话不存在。"))?;
        (saved.session.clone(), saved.view.clone())
    } else {
        (SelectionSession::default(), None)
    };
    next.record(&target, view.clone());
    next.persist()?;
    let workspace = Workspace {
        session: target.clone(),
        view,
        conversations: next.summaries(),
    };
    *current = target;
    *history = next;
    Ok(workspace)
}

fn manage_conversation(
    current: &mut SelectionSession,
    history: &mut History,
    view: ConversationView,
    target_id: &str,
    pinned: Option<bool>,
) -> AppResult<Workspace> {
    if current.running {
        return Err(validation("当前正在生成，请完成后再操作选型对话。"));
    }
    validate_view(&view)?;
    let mut next = history.clone();
    next.record(current, Some(view));
    let target = next
        .sessions
        .get_mut(target_id)
        .ok_or_else(|| validation("该选型对话不存在。"))?;
    let (session, view) = if let Some(pinned) = pinned {
        target.pinned = pinned;
        target.pin_updated_at_ms = now_ms().max(target.pin_updated_at_ms.saturating_add(1));
        (current.clone(), next.sessions[&current.id].view.clone())
    } else {
        next.sessions.remove(target_id);
        next.deleted_ids.insert(target_id.into());
        if target_id == current.id {
            let saved = next
                .summaries()
                .first()
                .and_then(|item| next.sessions.get(&item.id));
            saved
                .map(|saved| (saved.session.clone(), saved.view.clone()))
                .unwrap_or_else(|| (SelectionSession::default(), None))
        } else {
            (current.clone(), next.sessions[&current.id].view.clone())
        }
    };
    next.record(&session, view.clone());
    next.persist()?;
    let workspace = Workspace {
        session: session.clone(),
        view,
        conversations: next.summaries(),
    };
    *current = session;
    *history = next;
    Ok(workspace)
}

#[tauri::command]
pub(crate) fn selection_conversation_pin(
    session_id: String,
    revision: u64,
    target_id: String,
    pinned: bool,
    view: ConversationView,
    state: State<'_, SelectionState>,
) -> AppResult<Workspace> {
    let mut current = state.0.lock().map_err(|_| lock_error())?;
    version_check(&current, &session_id, revision)?;
    let mut history = state.1.lock().map_err(|_| lock_error())?;
    manage_conversation(&mut current, &mut history, view, &target_id, Some(pinned))
}

#[tauri::command]
pub(crate) fn selection_conversation_delete(
    session_id: String,
    revision: u64,
    target_id: String,
    view: ConversationView,
    state: State<'_, SelectionState>,
) -> AppResult<Workspace> {
    let mut current = state.0.lock().map_err(|_| lock_error())?;
    version_check(&current, &session_id, revision)?;
    let mut history = state.1.lock().map_err(|_| lock_error())?;
    manage_conversation(&mut current, &mut history, view, &target_id, None)
}

#[tauri::command]
pub(crate) fn selection_conversations(
    state: State<'_, SelectionState>,
) -> AppResult<ConversationIndex> {
    let current = state.0.lock().map_err(|_| lock_error())?;
    let mut history = state.1.lock().map_err(|_| lock_error())?.clone();
    if let Some(error) = &history.load_error {
        return Err(validation(error));
    }
    let view = history
        .sessions
        .get(&current.id)
        .and_then(|saved| saved.view.clone());
    if !history.sessions.contains_key(&current.id) {
        history.record(&current, view.clone());
    }
    Ok(ConversationIndex {
        view,
        conversations: history.summaries(),
    })
}

#[tauri::command]
pub(crate) fn selection_conversation_open(
    session_id: String,
    revision: u64,
    target_id: Option<String>,
    view: ConversationView,
    state: State<'_, SelectionState>,
) -> AppResult<Workspace> {
    let mut current = state.0.lock().map_err(|_| lock_error())?;
    version_check(&current, &session_id, revision)?;
    let mut history = state.1.lock().map_err(|_| lock_error())?;
    switch_conversation(&mut current, &mut history, view, target_id)
}

#[tauri::command]
pub(crate) fn selection_conversation_save(
    session_id: String,
    revision: u64,
    view: ConversationView,
    state: State<'_, SelectionState>,
) -> AppResult<Vec<ConversationSummary>> {
    validate_view(&view)?;
    let mut current = state.0.lock().map_err(|_| lock_error())?;
    version_check(&current, &session_id, revision)?;
    if current.running {
        return Err(validation("正在生成，稍后保存。"));
    }
    let mut history = state.1.lock().map_err(|_| lock_error())?;
    let mut next = history.clone();
    next.record(&current, Some(view));
    next.persist()?;
    let summaries = next.summaries();
    *history = next;
    current.storage_error.clear();
    Ok(summaries)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(card: RequirementCard) -> ConversationView {
        ConversationView {
            card: Some(card),
            description: "未发送的补充".into(),
            messages: vec!["原始需求".into()],
            editing: true,
        }
    }

    #[test]
    fn new_conversations_preserve_cards_reports_and_drafts_and_reload_from_disk() {
        let folder =
            std::env::temp_dir().join(format!("selection-history-test-{}", Uuid::new_v4()));
        let path = folder.join("history.json");
        let mut current = SelectionSession::default();
        let first = current.id.clone();
        let mut card = super::super::tests::card();
        card.tasks[0].fields.get_mut("input").unwrap().value = "尚未确认的编辑".into();
        current.description = card.description.clone();
        current.draft = Some(card.clone());
        current.task_runs.insert(card.tasks[0].id.clone(), TaskRun { selected_model: Some("model-a".into()), fallback_model: Some("model-b".into()), research: Some(serde_json::from_value(serde_json::json!({ "status":"complete", "revision":0, "researchedAtMs":1, "asOfDate":"2026-10-22", "sources":[], "report":{"groups":[{"taskId":card.tasks[0].id,"candidates":[{"model":"model-a","reason":"适配理由"}]}]}, "error":"" })).unwrap()) });
        let mut history = History {
            path: Some(path.clone()),
            ..Default::default()
        };
        let saved_view = view(card);
        let workspace =
            switch_conversation(&mut current, &mut history, saved_view.clone(), None).unwrap();
        assert_ne!(current.id, first);
        assert!(current.task_runs.is_empty());
        assert_eq!(workspace.conversations.len(), 2);
        let second = current.id.clone();
        switch_conversation(
            &mut current,
            &mut history,
            ConversationView {
                card: None,
                description: "第二个对话的草稿".into(),
                messages: vec![],
                editing: true,
            },
            Some(first.clone()),
        )
        .unwrap();
        assert_eq!(
            current
                .task_runs
                .values()
                .next()
                .unwrap()
                .selected_model
                .as_deref(),
            Some("model-a")
        );
        let loaded = SelectionState::load(path.clone()).unwrap();
        assert_eq!(loaded.0.lock().unwrap().id, first);
        let saved = loaded.1.lock().unwrap();
        assert_eq!(saved.sessions.len(), 2);
        assert_eq!(
            saved.sessions[&second].view.as_ref().unwrap().description,
            "第二个对话的草稿"
        );
        let restored = saved.sessions[&first].view.as_ref().unwrap();
        assert_eq!(restored.description, "未发送的补充");
        assert_eq!(
            restored.card.as_ref().unwrap().tasks[0].fields["input"].value,
            "尚未确认的编辑"
        );
        assert_eq!(
            saved.sessions[&first]
                .session
                .task_runs
                .values()
                .next()
                .unwrap()
                .fallback_model
                .as_deref(),
            Some("model-b")
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn failed_switch_or_save_never_replaces_the_current_conversation() {
        let mut current = SelectionSession::default();
        let first = current.id.clone();
        let mut history = History::default();
        let draft = view(super::super::tests::card());
        assert!(
            switch_conversation(
                &mut current,
                &mut history,
                draft.clone(),
                Some("missing".into())
            )
            .is_err()
        );
        assert_eq!(current.id, first);
        assert!(history.sessions.is_empty());
        current.running = true;
        assert!(switch_conversation(&mut current, &mut history, draft.clone(), None).is_err());
        current.running = false;
        let folder =
            std::env::temp_dir().join(format!("selection-history-failure-{}", Uuid::new_v4()));
        fs::create_dir_all(&folder).unwrap();
        history.path = Some(folder.clone());
        assert!(switch_conversation(&mut current, &mut history, draft, None).is_err());
        assert_eq!(current.id, first);
        assert!(history.sessions.is_empty());
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn restarting_clears_abandoned_running_state_without_starting_another_request() {
        let folder =
            std::env::temp_dir().join(format!("selection-history-recovery-{}", Uuid::new_v4()));
        let path = folder.join("history.json");
        let mut session = SelectionSession::default();
        session.running = true;
        let mut card = super::super::tests::card();
        card.tasks[0].fields.remove("taskDifficulty");
        card.tasks[0].fields.remove("taskFocus");
        session.draft = Some(card.clone());
        session.task_runs.insert("task".into(), TaskRun { research: Some(serde_json::from_value(serde_json::json!({"status":"running", "revision":0, "researchedAtMs":0, "asOfDate":"2026-10-22", "sources":[], "report":null, "error":""})).unwrap()), ..Default::default() });
        let mut history = History {
            path: Some(path.clone()),
            ..Default::default()
        };
        history.record(&session, Some(view(card)));
        history.persist().unwrap();
        let restored = SelectionState::load(path).unwrap();
        let current = restored.0.lock().unwrap();
        assert!(!current.running);
        assert!(current.progress.is_none());
        assert_eq!(
            current.task_runs["task"].research.as_ref().unwrap().status,
            "failed"
        );
        assert!(
            current.draft.as_ref().unwrap().tasks[0]
                .fields
                .contains_key("taskDifficulty")
        );
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn backend_checkpoint_restores_new_results_without_waiting_for_frontend_autosave() {
        let folder = std::env::temp_dir().join(format!("selection-checkpoint-{}", Uuid::new_v4()));
        let path = folder.join("history.json");
        let state = SelectionState::load(path.clone()).unwrap();
        let mut session = state.0.lock().unwrap();
        let mut draft = super::super::tests::card();
        draft.tasks[0].fields.get_mut("output").unwrap().value = "最新转写结果".into();
        session.draft = Some(draft);
        session.description = "本次需求".into();
        state.checkpoint(&mut session, true);
        assert!(session.storage_error.is_empty());
        let restored = SelectionState::load(path).unwrap();
        assert_eq!(
            restored.0.lock().unwrap().draft.as_ref().unwrap().tasks[0].fields["output"].value,
            "最新转写结果"
        );
        assert!(
            restored.1.lock().unwrap().sessions[&session.id]
                .view
                .is_none()
        );
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn concurrent_windows_do_not_overwrite_other_conversations() {
        let folder = std::env::temp_dir().join(format!("selection-concurrent-{}", Uuid::new_v4()));
        let path = folder.join("history.json");
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let threads: Vec<_> = (0..2)
            .map(|_| {
                let path = path.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let mut history = History {
                        path: Some(path),
                        ..Default::default()
                    };
                    history.record(&SelectionSession::default(), None);
                    barrier.wait();
                    history.persist().unwrap();
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(
            SelectionState::load(path)
                .unwrap()
                .1
                .lock()
                .unwrap()
                .sessions
                .len(),
            2
        );
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn pinning_survives_saves_reload_and_other_windows() {
        let folder = std::env::temp_dir().join(format!("selection-pin-{}", Uuid::new_v4()));
        let path = folder.join("history.json");
        let mut current = SelectionSession::default();
        let first = current.id.clone();
        let draft = view(super::super::tests::card());
        let mut history = History {
            path: Some(path.clone()),
            ..Default::default()
        };
        switch_conversation(&mut current, &mut history, draft.clone(), None).unwrap();
        let mut stale = history.clone();
        let mut stale_current = current.clone();
        let original_ids: BTreeSet<_> = history.sessions.keys().cloned().collect();
        manage_conversation(
            &mut current,
            &mut history,
            draft.clone(),
            &first,
            Some(true),
        )
        .unwrap();
        assert!(history.summaries()[0].pinned);
        assert_eq!(history.summaries()[0].id, first);
        assert_eq!(
            history.sessions.keys().cloned().collect::<BTreeSet<_>>(),
            original_ids
        );
        assert_eq!(
            history
                .summaries()
                .iter()
                .filter(|item| item.id == first)
                .count(),
            1
        );
        stale.record(&stale_current, Some(draft.clone()));
        stale.persist().unwrap();
        assert!(stale.sessions[&first].pinned);
        let reloaded = SelectionState::load(path.clone()).unwrap();
        assert!(reloaded.1.lock().unwrap().sessions[&first].pinned);
        manage_conversation(&mut stale_current, &mut stale, draft, &first, Some(false)).unwrap();
        assert!(
            !SelectionState::load(path)
                .unwrap()
                .1
                .lock()
                .unwrap()
                .sessions[&first]
                .pinned
        );
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn deleting_current_restores_another_draft_and_stale_windows_cannot_resurrect_it() {
        let folder = std::env::temp_dir().join(format!("selection-delete-{}", Uuid::new_v4()));
        let path = folder.join("history.json");
        let mut current = SelectionSession::default();
        let first = current.id.clone();
        let draft = view(super::super::tests::card());
        let mut history = History {
            path: Some(path.clone()),
            ..Default::default()
        };
        switch_conversation(&mut current, &mut history, draft.clone(), None).unwrap();
        let second = current.id.clone();
        let mut stale = history.clone();
        let workspace =
            manage_conversation(&mut current, &mut history, draft.clone(), &second, None).unwrap();
        assert_eq!(current.id, first);
        assert_eq!(workspace.view.unwrap().description, "未发送的补充");
        stale.active_id = first.clone();
        stale.persist().unwrap();
        assert!(!stale.sessions.contains_key(&second));
        let restored = SelectionState::load(path.clone()).unwrap();
        assert!(!restored.1.lock().unwrap().sessions.contains_key(&second));
        let workspace =
            manage_conversation(&mut current, &mut history, draft, &first, None).unwrap();
        assert_ne!(workspace.session.id, first);
        assert!(workspace.view.is_none());
        assert_eq!(workspace.conversations.len(), 1);
        assert!(workspace.session.draft.is_none());
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn failed_management_does_not_discard_the_current_draft() {
        let mut current = SelectionSession::default();
        let id = current.id.clone();
        let draft = view(super::super::tests::card());
        let mut history = History::default();
        assert!(
            manage_conversation(&mut current, &mut history, draft.clone(), "missing", None)
                .is_err()
        );
        assert!(history.sessions.is_empty());
        current.running = true;
        assert!(
            manage_conversation(&mut current, &mut history, draft.clone(), &id, Some(true))
                .is_err()
        );
        current.running = false;
        let folder =
            std::env::temp_dir().join(format!("selection-delete-failure-{}", Uuid::new_v4()));
        fs::create_dir_all(&folder).unwrap();
        history.path = Some(folder.clone());
        assert!(manage_conversation(&mut current, &mut history, draft, &id, None).is_err());
        assert_eq!(current.id, id);
        assert!(history.deleted_ids.is_empty());
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn corrupt_history_is_preserved_instead_of_overwritten() {
        let path =
            std::env::temp_dir().join(format!("selection-history-corrupt-{}", Uuid::new_v4()));
        fs::write(&path, b"broken json").unwrap();
        let state = SelectionState::load(path.clone()).unwrap();
        assert!(!state.0.lock().unwrap().running);
        assert!(state.1.lock().unwrap().persist().is_err());
        let mut current = state.0.lock().unwrap();
        state.checkpoint(&mut current, true);
        assert!(!current.storage_error.is_empty());
        assert_eq!(fs::read(&path).unwrap(), b"broken json");
        fs::remove_file(path).unwrap();
    }
}
