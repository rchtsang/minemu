use std::num::NonZeroU64;
use std::sync::mpsc::{Receiver, TryRecvError};

use minemu_runtime::{
    RuntimeError, RuntimeHandle, RuntimeInspection, RuntimeInspectionRequest, RuntimeStatus,
};

use super::types::WidgetId;

pub enum PendingResult {
    Ready {
        target: WidgetId,
        inspection: RuntimeInspection,
    },
    Failed {
        target: WidgetId,
        request: RuntimeInspectionRequest,
        error: String,
    },
}

struct PendingInspection {
    target: WidgetId,
    request: RuntimeInspectionRequest,
    receiver: Receiver<std::result::Result<RuntimeInspection, RuntimeError>>,
}

pub struct RuntimeController {
    handle: RuntimeHandle,
    status: RuntimeStatus,
    pending: Vec<PendingInspection>,
}

impl RuntimeController {
    pub fn new(handle: RuntimeHandle) -> Self {
        let status = handle.status();
        Self {
            handle,
            status,
            pending: Vec::new(),
        }
    }

    pub fn status(&self) -> &RuntimeStatus {
        &self.status
    }

    pub fn refresh_status(&mut self) -> Option<RuntimeStatus> {
        let status = self.handle.status();
        if status == self.status {
            return None;
        }
        self.status = status.clone();
        Some(status)
    }

    pub fn request(
        &mut self,
        target: WidgetId,
        request: RuntimeInspectionRequest,
    ) -> Result<(), RuntimeError> {
        if self
            .pending
            .iter()
            .any(|pending| pending.target == target && pending.request == request)
        {
            return Ok(());
        }
        let receiver = self.handle.request_inspection(request.clone())?;
        self.pending.push(PendingInspection {
            target,
            request,
            receiver,
        });
        Ok(())
    }

    pub fn poll(&mut self) -> Vec<PendingResult> {
        let mut results = Vec::new();
        let mut index = 0;
        while index < self.pending.len() {
            let result = match self.pending[index].receiver.try_recv() {
                Ok(Ok(inspection)) => Some(PendingResult::Ready {
                    target: self.pending[index].target,
                    inspection,
                }),
                Ok(Err(error)) => Some(PendingResult::Failed {
                    target: self.pending[index].target,
                    request: self.pending[index].request.clone(),
                    error: error.to_string(),
                }),
                Err(TryRecvError::Disconnected) => Some(PendingResult::Failed {
                    target: self.pending[index].target,
                    request: self.pending[index].request.clone(),
                    error: "inspection response channel closed".into(),
                }),
                Err(TryRecvError::Empty) => None,
            };
            if let Some(result) = result {
                self.pending.swap_remove(index);
                results.push(result);
            } else {
                index += 1;
            }
        }
        results
    }

    pub fn pause(&self) -> Result<(), RuntimeError> {
        self.handle.pause()
    }

    pub fn resume(&self, instruction_limit: Option<NonZeroU64>) -> Result<(), RuntimeError> {
        match instruction_limit {
            Some(limit) => self.handle.resume_for(limit),
            None => self.handle.resume(),
        }
    }

    pub fn reset(&self) -> Result<(), RuntimeError> {
        self.handle.reset()
    }

    pub fn send_uart(&self, port: minemu_runtime::UartPort, bytes: &[u8]) {
        self.handle.send_uart(port, bytes);
    }

    pub fn shutdown(&self) -> Result<(), RuntimeError> {
        self.handle.shutdown()
    }
}
