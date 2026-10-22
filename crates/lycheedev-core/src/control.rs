//! One immutable deadline and timestamped cancellation lineage per operation.
use crate::{Result,ToolkitError};
use std::{sync::{Arc,Mutex,atomic::{AtomicBool,Ordering}},time::{Duration,Instant}};
#[derive(Debug,Clone,Copy,PartialEq,Eq)]
pub enum StopCause { Cancelled,DeadlineExceeded }
#[derive(Debug)]
struct CancellationState { cancelled:AtomicBool,at:Mutex<Option<Instant>> }
#[derive(Debug,Clone)]
pub struct TrackedCancellation { state:Arc<CancellationState> }
impl Default for TrackedCancellation {fn default()->Self{Self::new()}}
impl TrackedCancellation {
    pub fn new()->Self {Self{state:Arc::new(CancellationState{cancelled:AtomicBool::new(false),at:Mutex::new(None)})}}
    pub fn cancel(&self) {
        let mut at=self.state.at.lock().unwrap_or_else(|poison|poison.into_inner());
        if at.is_none(){*at=Some(Instant::now());}
        self.state.cancelled.store(true,Ordering::Release);
    }
    pub fn is_cancelled(&self)->bool {self.state.cancelled.load(Ordering::Acquire)}
}
#[derive(Debug,Clone)]
pub struct OperationControl {
    deadline:Instant,
    lineage:Vec<TrackedCancellation>,
    first_stop:Arc<Mutex<Option<StopCause>>>,
    stopped:Arc<AtomicBool>,
}
impl Default for OperationControl {fn default()->Self{Self::uncancelable(Instant::now()+Duration::from_secs(180))}}
impl OperationControl {
    pub fn uncancelable(deadline:Instant)->Self {Self::tracked(deadline,TrackedCancellation::new())}
    pub fn tracked(deadline:Instant,cancellation:TrackedCancellation)->Self {Self{deadline,lineage:vec![cancellation],first_stop:Arc::new(Mutex::new(None)),stopped:Arc::new(AtomicBool::new(false))}}
    pub fn ordered_child(&self,deadline:Instant)->Result<Self> {
        self.stop_cause()?;
        let mut lineage=self.lineage.clone();lineage.push(TrackedCancellation::new());
        Ok(Self{deadline:deadline.min(self.deadline),lineage,first_stop:Arc::new(Mutex::new(None)),stopped:Arc::new(AtomicBool::new(false))})
    }
    pub fn tracked_cancel_handle(&self)->Result<TrackedCancellation> {Ok(self.lineage.last().expect("operation cancellation owner").clone())}
    /// Lock ancestors in root-to-leaf order. Reading the clock while those locks
    /// are held gives cancellation and deadline one monotonic event order.
    pub fn stop_cause(&self)->Result<Option<StopCause>> {
        if self.stopped.load(Ordering::Acquire){return Ok(*self.first_stop.lock().unwrap_or_else(|p|p.into_inner()));}
        if self.lineage.iter().all(|signal|!signal.is_cancelled()) && Instant::now()<self.deadline{return Ok(None);}
        let stamps:Vec<_>=self.lineage.iter().map(|signal|signal.state.at.lock().unwrap_or_else(|p|p.into_inner())).collect();
        let earliest=stamps.iter().filter_map(|at|**at).min();let now=Instant::now();
        let mut first=self.first_stop.lock().unwrap_or_else(|p|p.into_inner());
        if first.is_none(){*first=if earliest.is_some_and(|at|at<self.deadline){Some(StopCause::Cancelled)}else if now>=self.deadline{Some(StopCause::DeadlineExceeded)}else{None};}
        if first.is_some(){self.stopped.store(true,Ordering::Release);}
        Ok(*first)
    }
    pub fn check(&self)->Result<()> {match self.stop_cause()? {Some(StopCause::Cancelled)=>Err(ToolkitError::new("command.cancelled","operation cancelled by the caller",7)),Some(StopCause::DeadlineExceeded)=>Err(ToolkitError::new("command.deadline","operation deadline exhausted",5)),None=>Ok(())}}
    pub fn remaining(&self)->Result<Duration>{self.check()?;Ok(self.deadline.saturating_duration_since(Instant::now()))}
    pub fn deadline(&self)->Instant{self.deadline}
    pub fn is_cancelled(&self)->bool{self.lineage.iter().any(TrackedCancellation::is_cancelled)}
    pub fn same_scope(&self,other:&Self)->bool{Arc::ptr_eq(&self.first_stop,&other.first_stop)}
    pub fn clone_allocation_bytes(&self)->usize{self.lineage.len()*std::mem::size_of::<TrackedCancellation>()}
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn child_stop_preserves_parent_and_parent_stop_reaches_all_descendants(){
        let parent=OperationControl::default();let first=parent.ordered_child(parent.deadline()+Duration::from_secs(1)).unwrap();
        assert_eq!(first.deadline(),parent.deadline());
        first.tracked_cancel_handle().unwrap().cancel();
        assert_eq!(first.stop_cause().unwrap(),Some(StopCause::Cancelled));parent.check().unwrap();
        let second=parent.ordered_child(parent.deadline()).unwrap();let nested=second.ordered_child(second.deadline()).unwrap();
        parent.tracked_cancel_handle().unwrap().cancel();
        assert_eq!(nested.stop_cause().unwrap(),Some(StopCause::Cancelled));assert!(second.is_cancelled());
    }
    #[test]
    fn first_actual_event_wins_and_clones_retain_its_cause(){
        let signal=TrackedCancellation::new();let expired=OperationControl::tracked(Instant::now(),signal.clone());signal.cancel();
        assert_eq!(expired.stop_cause().unwrap(),Some(StopCause::DeadlineExceeded));
        assert_eq!(expired.clone().check().unwrap_err().code,"command.deadline");
        let alive=OperationControl::default();alive.tracked_cancel_handle().unwrap().cancel();
        assert_eq!(alive.check().unwrap_err().code,"command.cancelled");
    }
    #[test]
    fn child_retains_parent_signal_without_mutating_local_state(){
        let parent=OperationControl::default();let child=parent.ordered_child(parent.deadline()).unwrap();let local=child.tracked_cancel_handle().unwrap();
        parent.tracked_cancel_handle().unwrap().cancel();assert!(child.is_cancelled());assert!(!local.is_cancelled());
        assert!(!parent.same_scope(&child));assert!(child.same_scope(&child.clone()));
    }
}
