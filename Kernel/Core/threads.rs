// "Tifflin" Kernel
// - By John Hodge (thePowersGang)
//
// Core/threads.rs
// - Thread management
use _common::*;

use lib::mem::Rc;
use core::cell::RefCell;
use lib::Queue;

pub type ThreadHandle = Box<Thread>;

//#[deriving(PartialEq)]
enum RunState
{
	StateRunnable,
	StateListWait(*const WaitQueue),
	StateEventWait(u32),
	StateDead(u32),
}
impl Default for RunState { fn default() -> RunState { StateRunnable } }

#[deriving(Default)]
pub struct Thread
{
	//name: String,
	tid: uint,
	run_state: RunState,
	
	pub cpu_state: ::arch::threads::State,
	next: Option<Box<Thread>>,
}

pub struct WaitQueue
{
	list: ThreadList,
}
struct ThreadList
{
	first: Option<Box<Thread>>,
	last: Option<*const Thread>
}
pub const WAITQUEUE_INIT: WaitQueue = WaitQueue { list: ThreadList {first: None, last: None} };

// ----------------------------------------------
// Statics
//static s_all_threads:	::sync::Mutex<Map<uint,*const Thread>> = mutex_init!("s_all_threads", Map{});
static s_runnable_threads: ::sync::Spinlock<Queue<Box<Thread>>> = spinlock_init!(queue_init!());

// ----------------------------------------------
// Code
pub fn init()
{
	let tid0 = newthread(Thread {
		tid: 0,
		run_state: StateRunnable,
		cpu_state: ::arch::threads::init_tid0_state(),
		..Default::default()
		});
	::arch::threads::set_thread_ptr( tid0 )
}

pub fn newthread(t: Thread) -> Box<Thread>
{
	let rv = box t;
	
	// TODO: Add to global list of threads (removed on destroy)
	
	rv
}

pub fn yield_time()
{
	s_runnable_threads.lock().push( get_cur_thread() );
	reschedule();
}

fn reschedule()
{
	// 1. Get next thread
	log_trace!("reschedule()");
	let thread = get_thread_to_run();
	match thread
	{
	::core::option::None => {
		// Wait? How is there nothing to run?
		log_warning!("BUGCHECK: No runnable threads");
		},
	::core::option::Some(t) => {
		// 2. Switch to next thread
		log_debug!("Task switch to {:u}", t.tid);
		::arch::threads::switch_to(t);
		}
	}
}

fn get_cur_thread() -> Box<Thread>
{
	::arch::threads::get_thread_ptr().unwrap()
}
fn rel_cur_thread(t: Box<Thread>)
{
	::arch::threads::set_thread_ptr(t)
}

fn get_thread_to_run() -> Option<Box<Thread>>
{
	let mut handle = s_runnable_threads.lock();
	if handle.empty()
	{
		// WTF? At least an idle thread should be ready
		None
	}
	else
	{
		// 2. Pop off a new thread
		handle.pop()
	}
}

impl ThreadList
{
	pub fn empty(&self) -> bool
	{
		self.first.is_none()
	}
	pub fn pop(&mut self) -> Option<Box<Thread>>
	{
		match self.first.take()
		{
		Some(t) => {
			self.first = t.next.take();
			if self.first.is_none() {
				self.last = None;
			}
			Some(t)
			},
		None => None
		}
	}
	pub fn push(&mut self, t: Box<Thread>)
	{
		let ptr: *const Thread = unsafe { &*t };
		// 2. Tack thread onto end
		if self.first.is_some()
		{
			assert!(self.last.is_some());
			// Using unsafe and rawptr deref here is safe, because WaitQueue should be locked
			unsafe {
				let last_ref = &mut *self.last.unwrap();
				assert!(last_ref.next.is_none());
				last_ref.next = Some(t);
			}
		}
		else
		{
			assert!(self.last.is_none());
			self.first = Some(t);
		}
		self.last = Some(ptr);
	}
}

impl WaitQueue
{
	pub fn wait<'a>(&mut self, lock_handle: ::arch::sync::HeldSpinlock<'a,bool>)
	{
		// 1. Lock global list?
		let cur = get_cur_thread();
		assert!(cur.next.is_none());
		// - Keep rawptr kicking around for debug purposes
		cur.run_state = StateListWait(self as *mut _ as *const _);
		self.list.push(cur);
		// Unlock handle (short spinlocks disable interrupts)
		{ let _ = lock_handle; }
		// 4. Reschedule, and should return with state changed to run
		reschedule();
		
		let cur = get_cur_thread();
		assert!( !is!(cur.run_state, StateListWait(_)) );
		assert!( is!(cur.run_state, StateRunnable) );
		rel_cur_thread(cur);
	}
	pub fn wake_one(&mut self)
	{
		match self.list.pop()
		{
		Some(t) => {
			t.run_state = StateRunnable;
			s_runnable_threads.lock().push(t);
			},
		None => {}
		}
	}
}

// vim: ft=rust

