#[allow(unsafe_op_in_unsafe_fn)] 
use std::{alloc::{Layout, alloc, dealloc}, io::Error, mem::{self, MaybeUninit}, ptr, sync::atomic::{AtomicUsize, Ordering}};

/// Lock-free Round Queue with separated Reader and Writer
/// 
/// SAFETY: This is 100% unsafe internally, similar to tokio's design 
pub struct RWRoundQueue<T> {
    buffer: *mut MaybeUninit<T>,
    capacity: usize,
    layout: Layout,
    
    // Atomic indices for lock-free access
    read_idx: AtomicUsize,
    write_idx: AtomicUsize,
    
    // Cached pointers for optimization
    start_ptr: *const T,
    end_ptr: *const T, 
}  

unsafe impl<T: Send> Send for RWRoundQueue<T> {}
unsafe impl<T: Send> Sync for RWRoundQueue<T> {} 

impl<T> RWRoundQueue<T> {  

    /// Create a new read-write round queue with the specified size 
    pub fn new(capacity: usize) -> Result<Self, Error> { 
        if capacity == 0 {
            return Err(Error::new(std::io::ErrorKind::InvalidInput, "Capacity must be greater than 0"));
        } 
        if !capacity.is_power_of_two() {
            return Err(Error::new(std::io::ErrorKind::InvalidInput, "Capacity must be power of 2"));
        } 
        
        let layout = Layout::array::<MaybeUninit<T>>(capacity).unwrap();
        
        unsafe {
            let buffer = alloc(layout) as *mut MaybeUninit<T>;
            if buffer.is_null() {
                return Err(Error::new(std::io::ErrorKind::Other, "Memory allocation failed")); 
            }
            
            // Initialize memory with MaybeUninit pattern
            for i in 0..capacity {
                ptr::write(buffer.add(i), mem::zeroed());
            }
            
            let start_ptr = buffer as *const T;
            let end_ptr = buffer.add(capacity) as *const T;
            
            Ok(Self {
                buffer,
                capacity,
                layout,
                read_idx: AtomicUsize::new(0),
                write_idx: AtomicUsize::new(0),
                start_ptr,
                end_ptr,
            }) 
        } 
    } 

    /// Split into reader and writer
    /// 
    /// SAFETY: Only call once! Multiple splits will violate aliasing rules
    pub unsafe fn split(&mut self) -> (QueueReader<T>, QueueWriter<T>) {
        let reader = QueueReader {
            queue: self as *const Self,
            _phantom: std::marker::PhantomData,
        };
        
        let writer = QueueWriter {
            queue: self as *mut Self,
            _phantom: std::marker::PhantomData,
        };
        
        (reader, writer)
    } 

    #[inline]
    fn next_index(&self, current: usize) -> usize {
        (current + 1) & (self.capacity - 1)  // Fast modulo for power of 2
    }  

    // ===== Pointer Management Layer =====
    
    /// Acquire a read pointer (does NOT read the value)
    /// 
    /// Returns: pointer to the slot, or None if empty
    /// 
    /// SAFETY: 
    /// - Caller must ensure they call `commit_read()` after reading
    /// - Caller must not hold pointer across thread boundaries without sync
    pub unsafe fn acquire_read_ptr(&self) -> Option<*const MaybeUninit<T>> {
        let read_idx = self.read_idx.load(Ordering::Acquire);
        let write_idx = self.write_idx.load(Ordering::Acquire);
        
        // Empty check
        if read_idx == write_idx {
            return None;
        }
        
        // Return pointer to current read position
        let ptr = self.buffer.add(read_idx);
        Some(ptr as *const MaybeUninit<T>)
    }
    
    /// Commit the read operation (advances read pointer)
    /// 
    /// SAFETY:
    /// - Must be called after `acquire_read_ptr()`
    /// - Must not be called without acquiring first
    pub unsafe fn commit_read(&self) {
        let read_idx = self.read_idx.load(Ordering::Acquire);
        let next_read = self.next_index(read_idx);
        self.read_idx.store(next_read, Ordering::Release);
    }
    
    /// Acquire a write pointer (does NOT write the value)
    /// 
    /// Returns: (pointer, was_full) 
    /// - If was_full = true, caller should drop the old value first
    /// 
    /// SAFETY:
    /// - Caller must ensure they call `commit_write()` after writing
    /// - If was_full = true, caller must handle the old value
    pub unsafe fn acquire_write_ptr(&self) -> Option<(*mut MaybeUninit<T>, bool)> {
        let write_idx = self.write_idx.load(Ordering::Acquire);
        let read_idx = self.read_idx.load(Ordering::Acquire);
        let next_write = self.next_index(write_idx);
        
        // Check if full (would overtake read)
        let was_full = next_write == read_idx;
        
        let ptr = self.buffer.add(write_idx);
        Some((ptr, was_full))
    }
    
    /// Commit the write operation (advances write pointer)
    /// 
    /// If overwriting, also advances read pointer
    /// 
    /// SAFETY:
    /// - Must be called after `acquire_write_ptr()`
    /// - If was_full was true, caller must have handled old value
    pub unsafe fn commit_write(&self, was_full: bool) {
        let write_idx = self.write_idx.load(Ordering::Acquire);
        let next_write = self.next_index(write_idx);
        
        if was_full {
            // Advance read pointer (we're overwriting)
            let read_idx = self.read_idx.load(Ordering::Acquire);
            let next_read = self.next_index(read_idx);
            self.read_idx.store(next_read, Ordering::Release);
        }
        
        self.write_idx.store(next_write, Ordering::Release);
    }
    
    // ===== Helper Methods =====
    
    /// Get current length (approximate in concurrent context)
    pub fn len(&self) -> usize {
        let read_idx = self.read_idx.load(Ordering::Acquire);
        let write_idx = self.write_idx.load(Ordering::Acquire);
        
        if write_idx >= read_idx {
            write_idx - read_idx
        } else {
            self.capacity - read_idx + write_idx
        }
    }
    
    pub fn is_empty(&self) -> bool {
        let read_idx = self.read_idx.load(Ordering::Acquire);
        let write_idx = self.write_idx.load(Ordering::Acquire);
        read_idx == write_idx
    }
    
    pub fn capacity(&self) -> usize {
        self.capacity
    }
    
    // ===== High-level Convenience APIs =====
    
    /// Read value (convenience wrapper)
    pub unsafe fn try_read(&self) -> Option<T> {
        let ptr = self.acquire_read_ptr()?;
        let value = ptr::read(ptr).assume_init();
        self.commit_read();
        Some(value)
    }
    
    /// Write value with overwrite (convenience wrapper)
    pub unsafe fn write_overwrite(&self, value: T) -> bool {
        let (ptr, was_full) = self.acquire_write_ptr().unwrap();
        
        // Check whether we need to drop old value 
        // FIXME: We have no way to know whether T is init or not 
        // We just simply always drop if overwriting. This is not accurate and unsafe 
        // ptr::drop_in_place(ptr as *mut T);
        // FIXME: We don't drop the old value, which may cause memory leaks for non-Copy types 
        
        ptr::write(ptr, MaybeUninit::new(value));
        self.commit_write(was_full);
        
        was_full
    } 
} 

impl<T> Drop for RWRoundQueue<T> {
    fn drop(&mut self) {
        unsafe {
            // Drain remaining items
            while self.try_read().is_some() {}
            
            // Deallocate buffer
            dealloc(self.buffer as *mut u8, self.layout);
        }
    }
}

/// Reader handle (can be sent to another thread)
pub struct QueueReader<T> {
    queue: *const RWRoundQueue<T>,
    _phantom: std::marker::PhantomData<T>,
}

unsafe impl<T: Send> Send for QueueReader<T> {}

impl<T> QueueReader<T> {
    /// Read next item
    pub fn read(&self) -> Option<T> {
        unsafe {
            (*self.queue).try_read()
        }
    }
    
    /// Batch read up to `max` items
    pub fn read_batch(&self, max: usize) -> Vec<T> {
        let mut result = Vec::with_capacity(max);
        
        for _ in 0..max {
            if let Some(item) = self.read() {
                result.push(item);
            } else {
                break;
            }
        }
        
        result
    }
    
    pub fn len(&self) -> usize {
        unsafe { (*self.queue).len() }
    }
    
    pub fn is_empty(&self) -> bool {
        unsafe { (*self.queue).is_empty() }
    }
}

pub struct QueueWriter<T> {
    queue: *mut RWRoundQueue<T>,
    _phantom: std::marker::PhantomData<T>, 
}

unsafe impl<T: Send> Send for QueueWriter<T> {}

impl<T> QueueWriter<T> {
    /// Get pointer to next writable slot
    /// Returns: (pointer, was_full)
    pub unsafe fn acquire_ptr(&mut self) -> Option<(*mut MaybeUninit<T>, bool)> {
        (*self.queue).acquire_write_ptr()
    }
    
    /// Commit write operation
    pub unsafe fn commit(&mut self, was_full: bool) {
        (*self.queue).commit_write(was_full)
    }
    
    /// High-level write
    pub unsafe fn write(&mut self, value: T) -> bool {
        (*self.queue).write_overwrite(value)
    }
    
    pub fn capacity(&self) -> usize {
        unsafe { (*self.queue).capacity() }
    }
} 
