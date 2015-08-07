// "Tifflin" Kernel
// - By John Hodge (thePowersGang)
//
// Core/memory/phys.rs
// - Physical memory manager
use prelude::*;
//use arch::memory::addresses::{physinfo_start, physinfo_end};
use arch::memory::PAddr;

pub const NOPAGE : PAddr = 1;

static S_MEM_MAP: ::lib::LazyStatic<&'static [::memory::MemoryMapEnt]> = lazystatic_init!();
// S_MAPALLOC - Tracks the allocation point in S_MEM_MAP : (Entry Index, Address)
static S_MAPALLOC : ::sync::Mutex<(usize,PAddr)> = mutex_init!( (0,0) );
// TODO: Multiple stacks based on page colouring
static S_FREE_STACK : ::sync::Mutex<PAddr> = mutex_init!( NOPAGE );
// TODO: Reference counts (maybe require arch to expose that)

/// A handle to a physical page (maintaining a reference to it, even when not mapped)
pub struct FrameHandle(PAddr);

pub fn init()
{
	// 1. Acquire a memory map from the architecture code and save for use later
	// SAFE: Called in a true single-threaded context
	unsafe {
		S_MEM_MAP.prep(|| ::arch::boot::get_memory_map());
	}
	
	log_log!("Memory Map:");
	for (i,ent) in get_memory_map().iter().enumerate()
	{
		log_log!("#{} : {:?}", i, ent);
	}
}

impl FrameHandle
{
	/// UNSAFE due to using a raw physical address
	pub unsafe fn from_addr(addr: PAddr) -> FrameHandle {
		mark_used(addr);
		FrameHandle(addr)
	}
	/// UNSAFE due to using a raw physical address, and can cause a leak
	pub unsafe fn from_addr_noref(addr: PAddr) -> FrameHandle {
		FrameHandle(addr)
	}
	pub fn into_addr(self) -> PAddr {
		self.0
	}
}

fn get_memory_map() -> &'static [::memory::MemoryMapEnt]
{
	&*S_MEM_MAP
}

fn is_ram(phys: PAddr) -> bool
{
	for e in S_MEM_MAP.iter()
	{
		if e.start <= phys && phys < e.start + e.size
		{
			return match e.state
				{
				::memory::memorymap::MemoryState::Free => true,
				::memory::memorymap::MemoryState::Used => true,
				_ => false,
				};
		}
	}
	false
}

pub fn make_unique(page: PAddr) -> PAddr
{
	if !is_ram(page) {
		panic!("Calling 'make_unique' on non-RAM page");
	}
	else if ::arch::memory::phys::get_multiref_count(page) == 0 {
		page
	}
	else {
		todo!("make_unique");
	}
}

pub fn allocate_range_bits(bits: u8, count: usize) -> PAddr
{
	// XXX: HACK! Falls back to the simple code if possible
	if count == 1 && get_memory_map().last().unwrap().start >> bits == 0
	{
		return allocate_range(1);
	}
	// 1. Locate the last block of a suitable bitness
	// - Take care to correctly handle blocks that straddle bitness boundaries
	// NOTE: Memory map constructor _can_ break blocks up at common bitness boundaries (16, 24, 32 bits) to make this more efficient
	// 2. Obtain `count` pages from either the end (if possible) or the start of this block
	// TODO: If the block is not large enough, return an error (NOPAGE)
	panic!("TODO: allocate_range(bits={}, count={})", bits, count);
}

pub fn allocate_range(count: usize) -> PAddr
{
	if !(count == 1) {
		panic!("TODO: Large range allocations (count={})", count);
	}

	let mut h = S_MAPALLOC.lock();
	log_trace!("allocate_range: *h = {:?} (init)", *h);
	let (mut i,mut addr) = *h;
	
	let map = get_memory_map();
	if i == map.len() {
		log_error!("Out of physical memory");
		return NOPAGE;
	}
	if addr >= map[i].start + map[i].size
	{
		i += 1;
		while i != map.len() && map[i].state != ::memory::memorymap::MemoryState::Free {
			i += 1;
		}
		if i == map.len() {
			log_error!("Out of physical memory");
			*h = (i, 0);
			return NOPAGE;
		}
		addr = map[i].start;
	}
	let rv = addr;
	addr += ::PAGE_SIZE as u64;
	//log_trace!("allocate_range: rv={:#x}, i={}, addr={:#x}", rv, i, addr);
	*h = (i, addr);
	//log_trace!("allocate_range: *h = {:?}", *h);
	return rv;
}

pub fn allocate(address: *mut ()) -> bool
{
	log_trace!("allocate(address={:p})", address);
	// 1. Pop a page from the free stack
	unsafe
	{
		let mut h = S_FREE_STACK.lock();
		let paddr = *h;
		if paddr != NOPAGE
		{
			::memory::virt::map(address, paddr, super::virt::ProtectionMode::KernelRO);
			*h = *(address as *const PAddr);
			*(address as *mut [u8; ::PAGE_SIZE]) = ::core::mem::zeroed();
			mark_used(paddr);
			log_trace!("- {:p} (stack) paddr = {:#x}", address, paddr);
			return true;
		}
	}
	// 2. If none, allocate from map
	let paddr = allocate_range(1);
	if paddr != NOPAGE
	{
		// SAFE: Physical address just allocated
		unsafe {
			::memory::virt::map(address, paddr, super::virt::ProtectionMode::KernelRW);
			*(address as *mut [u8; ::PAGE_SIZE]) = ::core::mem::zeroed();
		}
		log_trace!("- {:p} (range) paddr = {:#x}", address, paddr);
		return true
	}
	// 3. Fail
	log_trace!("- (none)");
	false
}

pub fn ref_frame(paddr: PAddr)
{
	if ! is_ram(paddr) {
		
	}
	else {
		::arch::memory::phys::ref_frame(paddr / ::PAGE_SIZE as u64);
	}
}
pub fn deref_frame(paddr: PAddr)
{
	if ! is_ram(paddr) {
		log_log!("Calling deref_frame on non-RAM {:#x}", paddr);
	}
	// Dereference page (returns prevous value, zero meaning page was not multi-referenced)
	else if ::arch::memory::phys::deref_frame(paddr / ::PAGE_SIZE as u64) == 0 {
		// - This page is the only reference.
		if ::arch::memory::phys::mark_free(paddr / ::PAGE_SIZE as u64) == true {
			// Release frame back into the pool
			todo!("deref_frame({:#x}) Release", paddr);
		}
		else {
			// Page was either not allocated (oops) or is not managed
			// - Either way, ignore
		}
	}
}

fn mark_used(paddr: PAddr)
{
	log_error!("TODO: mark_used(paddr={:#x})", paddr);
	// TODO:
}

// vim: ft=rust
