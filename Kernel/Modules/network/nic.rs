// "Tifflin" Kernel - Networking Stack
// - By John Hodge (thePowersGang)
//
// Modules/network/nic.rs
//! "Network Interface Card" interface
use kernel::prelude::*;
use kernel::lib::mem::aref::{Aref,ArefBorrow};
use kernel::sync::Mutex;
use kernel::_async3 as async;

#[derive(Debug)]
pub enum Error
{
	/// No packets waiting
	NoPacket,
	/// An oversized packet was received
	MtuExceeded,
	/// Not enough space avaliable for the packet
	BufferUnderrun,
	///// Async stack space exceeded
	//AsyncTooDeep,
}

/// Chain of wrapping packet information, used for scatter-gather DMA
// TODO: Represent the lifetime of the components relative to the async root
// - Two lifetime parameters, one for inner and one for outer
pub struct SparsePacket<'a>
{
	head: &'a [u8],
	next: Option<&'a SparsePacket<'a>>,
}
impl<'a> SparsePacket<'a>
{
	pub fn new_root(data: &'a [u8]) -> SparsePacket<'a> {
		SparsePacket {
			head: data,
			next: None,
			}
	}
	pub fn new_chained(data: &'a [u8], next: &'a SparsePacket<'a>) -> SparsePacket<'a> {
		SparsePacket {
			head: data,
			next: Some(next),
			}
	}

	pub fn total_len(&self) -> usize {
		let mut s = self;
		let mut rv = 0;
		loop {
			rv += s.head.len();
			match s.next
			{
			None => break,
			Some(v) => s = v,
			}
		}
		rv
	}
}
impl<'a> IntoIterator for &'a SparsePacket<'a>
{
	type IntoIter = SparsePacketIter<'a>;
	type Item = &'a [u8];
	fn into_iter(self) -> SparsePacketIter<'a> {
		SparsePacketIter(Some(self))
	}
}
pub struct SparsePacketIter<'a>(Option<&'a SparsePacket<'a>>);
impl<'a> Iterator for SparsePacketIter<'a> {
	type Item = &'a [u8];
	fn next(&mut self) -> Option<Self::Item> {
		let p = match self.0
			{
			None => return None,
			Some(p) => p,
			};

		self.0 = p.next;
		Some(p.head)
	}
}

/// Handle to a packet in driver-owned memory
pub type PacketHandle<'a> = ::stack_dst::ValueA<RxPacket + 'a, [usize; 8]>;
/// Trait representing a packet in driver-owned memory
pub trait RxPacket
{
	fn len(&self) -> usize;
	fn num_regions(&self) -> usize;
	fn get_region(&self, idx: usize) -> &[u8];
	fn get_slice(&self, range: ::core::ops::Range<usize>) -> Option<&[u8]>;
}
#[derive(Clone)]
pub struct PacketReader<'a> {
	pkt: &'a PacketHandle<'a>,
	ofs: usize,
}
impl<'a> PacketReader<'a> {
	fn new(pkt: &'a PacketHandle<'a>) -> PacketReader<'a> {
		PacketReader {
			pkt: pkt,
			ofs: 0,
			}
	}
	pub fn remain(&self) -> usize {
		self.pkt.len() - self.ofs
	}
	pub fn read(&mut self, dst: &mut [u8]) -> Result<usize, ()> {
		// TODO: Should this be cached?
		let mut ofs = self.ofs;
		let mut r = 0;
		while ofs >= self.pkt.get_region(r).len() {
			ofs -= self.pkt.get_region(r).len();
			r += 1;
			if r == self.pkt.num_regions() {
				return Err( () );
			}
		}

		let mut wofs = 0;
		while wofs < dst.len() && self.ofs + wofs < self.pkt.len()
		{
			let rgn = self.pkt.get_region(r);
			let alen = rgn.len() - ofs;
			let rlen = dst.len() - wofs;
			let len = ::core::cmp::min(alen, rlen);

			dst[wofs..][..len].copy_from_slice( &rgn[ofs..][..len] );
			
			r += 1;
			ofs = 0;
			wofs += len;
		}

		self.ofs += wofs;
		Ok(wofs)
	}

	pub fn read_bytes<T: AsMut<[u8]>>(&mut self, mut b: T) -> Result<T, ()> {
		self.read(b.as_mut())?;
		Ok(b)
	}
	pub fn read_u8(&mut self) -> Result<u8, ()> {
		let mut b = [0];
		self.read(&mut b)?;
		Ok( b[0] )
	}
	pub fn read_u16n(&mut self) -> Result<u16, ()> {
		let mut b = [0,0];
		self.read(&mut b)?;
		Ok( (b[0] as u16) << 8 | (b[1] as u16) )
	}
	pub fn read_u32n(&mut self) -> Result<u32, ()> {
		let mut b = [0,0,0,0];
		self.read(&mut b)?;
		Ok( (b[0] as u32) << 24 + (b[1] as u32) << 16 | (b[2] as u32) << 8 | (b[3] as u32) )
	}
}

/// Network interface API
pub trait Interface: 'static + Send + Sync
{
	/// Transmit a raw packet (blocking)
	fn tx_raw(&self, pkt: SparsePacket);

	/// The input buffer can be a mix of `> 'stack` and `< 'stack` buffers. This function should collapse shorter lifetime
	/// buffers into an internal buffer that lives long enough.
	fn tx_async<'a, 's>(&'s self, async: async::ObjectHandle, stack: async::StackPush<'a, 's>, pkt: SparsePacket) -> Result<(), Error>;

	/// Called once to allow the interface to get an object to signal a new packet arrival
	fn rx_wait_register(&self, channel: &::kernel::threads::SleepObject);
	
	/// Obtain a packet from the interface (or `Err(Error::NoPacket)` if there is none)
	/// - Non-blocking
	fn rx_packet(&self) -> Result<PacketHandle, Error>;
}

struct InterfaceData
{
	#[allow(dead_code)]	// Never read, just exists to hold the handle
	base_interface: Aref<Interface+'static>,
	// TODO: Metadata?
	thread: ::kernel::threads::WorkerThread,
}

static INTERFACES_LIST: Mutex<Vec< Option<InterfaceData> >> = Mutex::new(Vec::new_const());

/// Handle to a registered interface
pub struct Registration<T> {
	// Logically owns the `T`
	pd: ::core::marker::PhantomData<T>,
	index: usize,
	ptr: ArefBorrow<T>,
}
impl<T> Drop for Registration<T> {
	fn drop(&mut self) {
		let mut lh = INTERFACES_LIST.lock();
		assert!( self.index < lh.len() );
		if let Some(ref int_ent) = lh[self.index] {
			//int_ent.stop_signal.set();
			int_ent.thread.wait().expect("Couldn't wait for NIC worker to terminate");
			// TODO: Inform the rest of the stack that this interface is gone?
		}
		else {
			panic!("NIC registration pointed to unpopulated entry");
		}
		lh[self.index] = None;
	}
}
impl<T> ::core::ops::Deref for Registration<T> {
	type Target = T;
	fn deref(&self) -> &T {
		&*self.ptr
	}
}

pub fn register<T: Interface>(mac_addr: [u8; 6], int: T) -> Registration<T> {
	let reg = Aref::new(int);

	// HACK: Send a dummy packet
	// - An ICMP Echo request to qemu's user network router (10.0.2.2 from 10.0.2.15)
	{
		// TODO: Make this a ARP lookup instead.
		let mut pkt = 
			//  MAC Dst                MAC Src     EtherTy IP      TotalLen Identif Frag   TTL Prot CkSum  Source          Dest            ICMP
			//*b"\xFF\xFF\xFF\xFF\xFF\xFF\0\0\0\0\0\0\x08\x00\x45\x00\x00\x23\x00\x00\x00\x00\xFF\x01\xa3\xca\x0A\x00\x02\x0F\x0A\x00\x02\x02\x08\x00\x7d\x0d\x00\x00\x00\x00Hello World"
			//  MAC Dst                MAC Src     EtherTy HWType  |Type   |sizes  |Req    |SourceMac              |SourceIP       |DestMac                |DestIP         |
			*b"\xFF\xFF\xFF\xFF\xFF\xFF\0\0\0\0\0\0\x08\x06\x00\x01\x08\x00\x06\x04\x00\x01\x52\x54\x00\x12\x34\x56\x0a\x00\x02\x0F\xFF\xFF\xFF\xFF\xFF\xFF\x0A\x00\x02\x02"
			;
		pkt[6..][..6].copy_from_slice( &mac_addr );

		// Blocking
		log_debug!("TESTING - Tx Blocking");
		reg.tx_raw(SparsePacket { head: &pkt, next: None });

		// Async
		log_debug!("TESTING - Tx Async");
		let mut o: async::Object = Default::default();
		reg.tx_async(o.get_handle(), o.get_stack(), SparsePacket { head: &pkt, next: None }).expect("Failed tx_async in testing");
		let h = [&o];
		{
			let w = async::Waiter::new(&h);
			w.wait_one();
		}
		log_debug!("TESTING - Tx Complete");
	}

	let worker_reg_handle = reg.borrow();
	let rv_reg_handle = reg.borrow();
	let reg = InterfaceData {
		thread: ::kernel::threads::WorkerThread::new("Network Rx", move || rx_thread(&*worker_reg_handle)),
		base_interface: reg,
		};

	fn insert_opt<T>(list: &mut Vec<Option<T>>, val: T) -> usize {
		for (i,s) in list.iter_mut().enumerate() {
			if s.is_none() {
				*s = Some(val);
				return i;
			}
		}
		list.push( Some(val) );
		return list.len() - 1;
	}
	let idx = insert_opt(&mut INTERFACES_LIST.lock(), reg);
	
	Registration {
		pd: ::core::marker::PhantomData,
		index: idx,
		ptr: rv_reg_handle,
		}
}

fn rx_thread(int: &Interface)
{
	let so = ::kernel::threads::SleepObject::new("rx_thread");
	int.rx_wait_register(&so);
	loop
	{
		so.wait();
		match int.rx_packet()
		{
		Ok(pkt) => {
			log_notice!("Received packet, len={} (chunks={})", pkt.len(), pkt.num_regions());
			for r in 0 .. pkt.num_regions() {
				log_debug!("{} {:?}", r, ::kernel::logging::HexDump(pkt.get_region(r)));
			}
			// TODO: Should this go in is own module?
			// 1. Interpret the `Ethernet II` header
			if pkt.len() < 6+6+2 {
				log_notice!("Short packet ({} < {})", pkt.len(), 6+6+2);
				continue ;
			}
			let mut r = PacketReader::new(&pkt);
			// 2. Hand off to sub-modules depending on the EtherTy field
			let src_mac = {
				let mut b = [0; 6];
				r.read(&mut b).unwrap();
				b
				};
			let _dst_mac = {
				let mut b = [0; 6];
				r.read(&mut b).unwrap();
				b
				};
			let ether_ty = r.read_u16n().unwrap();
			match ether_ty
			{
			0x0800 => ::ipv4::handle_rx_ethernet(int, src_mac, r).expect("Unable to hanle IPv4 packet (TODO)"),
			// ARP
			0x0806 => {
				// TODO: Pass on to ARP
				//::arp::handle_packet(int, r);
				// TODO: Length test
				let hw_ty  = r.read_u16n().unwrap();
				let sw_ty  = r.read_u16n().unwrap();
				let hwsize = r.read_u8().unwrap();
				let swsize = r.read_u8().unwrap();
				let code = r.read_u16n().unwrap();
				log_debug!("ARP HW {:04x} {}B SW {:04x} {}B req={}", hw_ty, hwsize, sw_ty, swsize, code);
				if hwsize == 6 {
					let mac = {
						let mut b = [0; 6];
						r.read(&mut b).unwrap();
						b
						};
					log_debug!("ARP HW {:?}", ::kernel::logging::HexDump(&mac));
				}
				if swsize == 4 {
					let ip = {
						let mut b = [0; 4];
						r.read(&mut b).unwrap();
						b
						};
					log_debug!("ARP SW {:?}", ip);
				}
				},
			v @ _ => {
				log_warning!("TODO: Handle packet with EtherTy={:#x}", v);
				},
			}
			},
		Err(Error::NoPacket) => {},
		Err(e) => todo!("{:?}", e),
		}
	}
}

