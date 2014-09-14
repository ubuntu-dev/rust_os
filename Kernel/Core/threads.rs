// "Tifflin" Kernel
// - By John Hodge (thePowersGang)
//
// Core/threads.rs
// - Thread management
use _common::*;

use lib::mem::Rc;
use core::cell::RefCell;
use lib::Queue;

pub type ThreadHandle = Rc<RefCell<Thread>>;

#[deriving(PartialEq)]
enum RunState
{
	StateRunnable,
	StateEventWait(u32),
	StateDead(u32),
}
impl Default for RunState { fn default() -> RunState { StateRunnable } }

#[deriving(Default)]
struct Thread
{
	//name: String,
	tid: uint,
	run_state: RunState,
	
	cpu_state: ::arch::threads::State,
}

pub struct WaitQueue
{
	first: Option<ThreadHandle>,
	last: Option<ThreadHandle>
}
pub static WAITQUEUE_INIT: WaitQueue = WaitQueue {first: None, last: None};

// ----------------------------------------------
// Statics
//static s_all_threads:	::sync::Mutex<Map<ThreadHandle>> = mutex_init!("s_all_threads", Map{});
static mut s_runnable_threads: ::sync::Spinlock<Queue<ThreadHandle>> = spinlock_init!(queue_init!());

// ----------------------------------------------
// Code
pub fn init()
{
	let tid0 = Rc::new( RefCell::new(Thread {
		tid: 0,
		run_state: StateRunnable,
		..Default::default()
		}) );
	::arch::threads::set_thread_ptr( tid0 )
}

pub fn reschedule()
{
	// 1. Get next thread
	log_trace!("reschedule()");
	let thread = get_thread_to_run();
	log_trace!("thread grabbed");
	match thread
	{
	::core::option::None => {
		// Wait? How is there nothing to run?
		log_debug!("it's none");
		},
	::core::option::Some(t) => {
		// 2. Switch to next thread
		log_debug!("Task switch to {:u}", t.borrow().tid);
		::arch::threads::switch_to(&t.borrow().cpu_state);
		}
	}
}

fn get_cur_thread() -> Rc<RefCell<Thread>>
{
	::arch::threads::get_thread_ptr()
}

fn get_thread_to_run() -> Option<Rc<RefCell<Thread>>>
{
	unsafe {
		log_trace!("Acquiring lock");
		let mut handle = s_runnable_threads.lock();
		log_trace!("Lock acquired");
		let cur = get_cur_thread();
		log_trace!("Cur grabbed");
		// 1. Put current thread on run queue (if needed)
		if cur.borrow().run_state == StateRunnable
		{
			handle.push(cur);
		}
		// 2. Pop off a new thread
		handle.pop()
	}
}

// vim: ft=rust

