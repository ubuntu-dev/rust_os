//
//
//
use core::mem::size_of;
use heap::Allocation;

pub struct ArrayAlloc<T>
{
	base: Allocation<T>,
	size: usize,
}

impl<T> ArrayAlloc<T>
{
	pub fn new(size: usize) -> ArrayAlloc<T> {
		//kernel_log!("ArrayAlloc::<{}>::new({})", type_name!(T), size);
		if size_of::<T>() == 0 {
			ArrayAlloc {
				// SAFE: Zero size is always valid
				base: unsafe { Allocation::new(0).expect("ArrayAlloc::new") },
				size: !0,
			}
		}
		else {
			ArrayAlloc {
				// SAFE: Upper level code ensures size is correct
				base: unsafe { Allocation::new( Self::get_alloc_size(size) ).expect("ArrayAlloc::new") },
				size: size,
			}
		}
	}
	pub unsafe fn from_raw_parts(base: *mut T, size: usize) -> ArrayAlloc<T> {
		ArrayAlloc {
			base: Allocation::from_raw(base),
			size: size,
		}
	}

	fn get_alloc_size(cap: usize) -> usize {
		cap * size_of::<T>()
	}
	
	pub fn resize(&mut self, newsize: usize) -> bool {
		//kernel_log!("ArrayAlloc::<{}>::resize({})", type_name!(T), newsize);
		// SAFE: This struct only exposes raw pointers, so any size is valid
		if unsafe { self.base.try_resize(newsize * size_of::<T>()) } {
			self.size = newsize;
			// Oh, good
			true
		}
		else {
			todo!("ArrayAlloc::expand({})", newsize);
		}
	}

	pub fn count(&self) -> usize {
		self.size
	}
	pub fn get_base(&self) -> *const T {
		*self.base
	}
	pub fn get_base_mut(&mut self) -> *mut T {
		*self.base
	}
	pub fn get_ptr(&self, idx: usize) -> *const T {
		// SAFE: Bounds checked
		unsafe {
			assert!(idx < self.size);
			self.base.offset( idx as isize )
		}
	}
	pub fn get_ptr_mut(&mut self, idx: usize) -> *mut T {
		// SAFE: Bounds checked
		unsafe {
			assert!(idx < self.size);
			self.base.offset( idx as isize )
		}
	}
}
