#![allow(unsafe_op_in_unsafe_fn)]
use std::{
    alloc::{alloc, dealloc, Layout},
    io::Error,
    mem::{self, MaybeUninit},
    ptr,
    sync::atomic::{AtomicUsize, Ordering},
};

/// Lock-free Round Queue with separated Reader and Writer
///
/// A high-performance circular buffer that allows lock-free concurrent access
/// between a single reader and a single writer. The queue uses atomic operations
/// for synchronization and automatically overwrites old data when full.
///
/// # Safety
///
/// This is 100% unsafe internally, similar to tokio's design. The implementation
/// uses raw pointers and atomic operations. Users must ensure proper synchronization
/// when splitting into reader/writer handles.
///
/// # Requirements
///
/// - Capacity must be a power of 2 for efficient modulo operations
/// - Type `T` must implement `Copy` for safe atomic operations
/// - Only split once - multiple splits violate aliasing rules
///
/// # Examples
///
/// ```
/// use os_lib::queue::RWRoundQueue;
///
/// // Create a queue with capacity 4
/// let mut queue = RWRoundQueue::<u32>::new(4).unwrap();
///
/// // Write some values
/// unsafe {
///     queue.write_overwrite(1);
///     queue.write_overwrite(2);
///     queue.write_overwrite(3);
/// }
///
/// // Read values back
/// unsafe {
///     assert_eq!(queue.try_read(), Some(1));
///     assert_eq!(queue.try_read(), Some(2));
///     assert_eq!(queue.try_read(), Some(3));
///     assert_eq!(queue.try_read(), None); // Empty
/// }
/// ```
///
/// # Overwrite Behavior
///
/// ```
/// use os_lib::queue::RWRoundQueue;
///
/// let mut queue = RWRoundQueue::<u32>::new(4).unwrap();
///
/// unsafe {
///     // Fill the queue (capacity-1 slots usable due to circular buffer design)
///     queue.write_overwrite(1);
///     queue.write_overwrite(2);
///     queue.write_overwrite(3);
///
///     // This overwrites the oldest value (1)
///     let was_full = queue.write_overwrite(4);
///     assert!(was_full);
///
///     // First value is now 2, not 1
///     assert_eq!(queue.try_read(), Some(2));
/// }
/// ```
pub struct RWRoundQueue<T: Copy> {
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

unsafe impl<T: Copy + Send> Send for RWRoundQueue<T> {}
unsafe impl<T: Copy + Send> Sync for RWRoundQueue<T> {}

impl<T: Copy> RWRoundQueue<T> {
    /// Creates a new read-write round queue with the specified capacity.
    ///
    /// # Arguments
    ///
    /// * `capacity` - Must be a power of 2 and greater than 0
    ///
    /// # Returns
    ///
    /// Returns `Ok(RWRoundQueue)` on success, or `Err` if:
    /// - Capacity is 0
    /// - Capacity is not a power of 2
    /// - Memory allocation fails
    ///
    /// # Examples
    ///
    /// ```
    /// use os_lib::queue::RWRoundQueue;
    ///
    /// // Valid: power of 2
    /// let queue = RWRoundQueue::<u32>::new(8).unwrap();
    /// assert_eq!(queue.capacity(), 8);
    ///
    /// // Invalid: not power of 2
    /// let result = RWRoundQueue::<u32>::new(7);
    /// assert!(result.is_err());
    ///
    /// // Invalid: zero capacity
    /// let result = RWRoundQueue::<u32>::new(0);
    /// assert!(result.is_err());
    /// ```
    pub fn new(capacity: usize) -> Result<Self, Error> {
        if capacity == 0 {
            return Err(Error::new(
                std::io::ErrorKind::InvalidInput,
                "Capacity must be greater than 0",
            ));
        }
        if !capacity.is_power_of_two() {
            return Err(Error::new(
                std::io::ErrorKind::InvalidInput,
                "Capacity must be power of 2",
            ));
        }

        let layout = Layout::array::<MaybeUninit<T>>(capacity).unwrap();

        unsafe {
            let buffer = alloc(layout) as *mut MaybeUninit<T>;
            if buffer.is_null() {
                return Err(Error::new(
                    std::io::ErrorKind::Other,
                    "Memory allocation failed",
                ));
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

    /// Splits the queue into separate reader and writer handles.
    ///
    /// This allows the reader and writer to be moved to different threads for
    /// lock-free concurrent access. The reader can only read, and the writer
    /// can only write, preventing data races.
    ///
    /// # Safety
    ///
    /// **WARNING**: Only call once! Multiple splits will violate aliasing rules
    /// and cause undefined behavior. Both handles point to the same underlying
    /// queue, so dropping the original queue while handles exist is unsafe.
    ///
    /// # Examples
    ///
    /// ```
    /// use os_lib::queue::RWRoundQueue;
    /// use std::{sync::mpsc, thread};
    ///
    /// let mut queue = RWRoundQueue::<u32>::new(8).unwrap();
    ///
    /// unsafe {
    ///     let (reader, mut writer) = queue.split();
    ///
    ///     let (start_tx, start_rx) = mpsc::channel();
    ///
    ///     // Reader can be moved to another thread
    ///     let read_handle = thread::spawn(move || {
    ///         start_rx.recv().unwrap();
    ///
    ///         let mut sum = 0;
    ///         for _ in 0..5 {
    ///             sum += reader.read().unwrap();
    ///         }
    ///         sum
    ///     });
    ///
    ///     // Writer stays in this thread
    ///     for i in 0..5 {
    ///         writer.write(i);
    ///     }
    ///     start_tx.send(()).unwrap();
    ///
    ///     let sum = read_handle.join().unwrap();
    ///     assert_eq!(sum, 0 + 1 + 2 + 3 + 4);
    /// }
    /// ```
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
        (current + 1) & (self.capacity - 1) // Fast modulo for power of 2
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
    /// - If was_full = true, caller is overwriting an older slot
    ///
    /// SAFETY:
    /// - Caller must ensure they call `commit_write()` after writing
    /// - If was_full = true, caller is overwriting an older slot
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
    /// - If was_full was true, caller overwrote an older slot
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

    /// Returns the current number of elements in the queue.
    ///
    /// # Note
    ///
    /// This value is approximate in concurrent contexts, as the reader or writer
    /// may be modifying the queue simultaneously. Use for monitoring/debugging
    /// purposes rather than exact synchronization.
    ///
    /// # Examples
    ///
    /// ```
    /// use os_lib::queue::RWRoundQueue;
    ///
    /// let mut queue = RWRoundQueue::<u32>::new(4).unwrap();
    ///
    /// unsafe {
    ///     assert_eq!(queue.len(), 0);
    ///     queue.write_overwrite(10);
    ///     queue.write_overwrite(20);
    ///     assert_eq!(queue.len(), 2);
    ///
    ///     queue.try_read();
    ///     assert_eq!(queue.len(), 1);
    /// }
    /// ```
    pub fn len(&self) -> usize {
        let read_idx = self.read_idx.load(Ordering::Acquire);
        let write_idx = self.write_idx.load(Ordering::Acquire);

        if write_idx >= read_idx {
            write_idx - read_idx
        } else {
            self.capacity - read_idx + write_idx
        }
    }

    /// Returns `true` if the queue is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use os_lib::queue::RWRoundQueue;
    ///
    /// let mut queue = RWRoundQueue::<u32>::new(4).unwrap();
    ///
    /// unsafe {
    ///     assert!(queue.is_empty());
    ///     queue.write_overwrite(42);
    ///     assert!(!queue.is_empty());
    /// }
    /// ```
    pub fn is_empty(&self) -> bool {
        let read_idx = self.read_idx.load(Ordering::Acquire);
        let write_idx = self.write_idx.load(Ordering::Acquire);
        read_idx == write_idx
    }

    /// Returns the maximum capacity of the queue.
    ///
    /// This value never changes after queue creation.
    ///
    /// # Examples
    ///
    /// ```
    /// use os_lib::queue::RWRoundQueue;
    ///
    /// let queue = RWRoundQueue::<u32>::new(16).unwrap();
    /// assert_eq!(queue.capacity(), 16);
    /// ```
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Returns the raw pointer to the start of the buffer.
    ///
    /// # Safety
    ///
    /// Caller must ensure the pointer is not used after the queue is dropped.
    pub unsafe fn start_ptr(&self) -> *const T {
        self.start_ptr
    }

    /// Returns the raw pointer to the end of the buffer.
    ///
    /// # Safety
    ///
    /// Caller must ensure the pointer is not used after the queue is dropped.
    pub unsafe fn end_ptr(&self) -> *const T {
        self.end_ptr
    }

    // ===== High-level Convenience APIs =====

    /// Attempts to read a value from the queue.
    ///
    /// This is a convenience wrapper around `acquire_read_ptr()` and `commit_read()`.
    /// Returns `None` if the queue is empty.
    ///
    /// # Safety
    ///
    /// Safe to call from reader thread. Caller must ensure proper synchronization
    /// in concurrent contexts.
    ///
    /// # Examples
    ///
    /// ```
    /// use os_lib::queue::RWRoundQueue;
    ///
    /// let mut queue = RWRoundQueue::<u32>::new(4).unwrap();
    ///
    /// unsafe {
    ///     queue.write_overwrite(100);
    ///     queue.write_overwrite(200);
    ///
    ///     assert_eq!(queue.try_read(), Some(100));
    ///     assert_eq!(queue.try_read(), Some(200));
    ///     assert_eq!(queue.try_read(), None);
    /// }
    /// ```
    pub unsafe fn try_read(&self) -> Option<T> {
        let ptr = self.acquire_read_ptr()?;
        let value = ptr::read(ptr).assume_init();
        self.commit_read();
        Some(value)
    }

    /// Writes a value to the queue, overwriting old data if full.
    ///
    /// This is a convenience wrapper around `acquire_write_ptr()` and `commit_write()`.
    /// When the queue is full, this automatically overwrites the oldest value and
    /// advances the read pointer.
    ///
    /// # Arguments
    ///
    /// * `value` - The value to write
    ///
    /// # Returns
    ///
    /// Returns `true` if an old value was overwritten, `false` otherwise.
    ///
    /// # Safety
    ///
    /// Safe to call from writer thread. Caller must ensure proper synchronization
    /// in concurrent contexts.
    ///
    /// # Examples
    ///
    /// ```
    /// use os_lib::queue::RWRoundQueue;
    ///
    /// let mut queue = RWRoundQueue::<u32>::new(4).unwrap();
    ///
    /// unsafe {
    ///     // First writes don't overwrite (capacity-1 slots are usable)
    ///     assert_eq!(queue.write_overwrite(1), false);
    ///     assert_eq!(queue.write_overwrite(2), false);
    ///     assert_eq!(queue.write_overwrite(3), false);
    ///
    ///     // Queue is now full, next write overwrites
    ///     assert_eq!(queue.write_overwrite(4), true);
    ///
    ///     // First value is now 2, not 1
    ///     assert_eq!(queue.try_read(), Some(2));
    /// }
    /// ```
    pub unsafe fn write_overwrite(&self, value: T) -> bool {
        let (ptr, was_full) = self.acquire_write_ptr().unwrap();

        ptr::write(ptr, MaybeUninit::new(value));
        self.commit_write(was_full);

        was_full
    }
}

impl<T: Copy> Drop for RWRoundQueue<T> {
    fn drop(&mut self) {
        unsafe {
            // No drop needed for Copy types.
            dealloc(self.buffer as *mut u8, self.layout);
        }
    }
}

/// Reader handle for lock-free queue access.
///
/// Created by calling [`RWRoundQueue::split()`]. This handle can be sent to
/// another thread to enable concurrent reading while a [`QueueWriter`] writes
/// from a different thread.
///
/// # Thread Safety
///
/// `QueueReader` implements `Send` (but not `Sync`), allowing it to be moved
/// to another thread. Only one reader should exist per queue.
///
/// # Examples
///
/// ```
/// use os_lib::queue::RWRoundQueue;
///
/// let mut queue = RWRoundQueue::<u32>::new(8).unwrap();
///
/// unsafe {
///     let (reader, mut writer) = queue.split();
///
///     // Write some data
///     writer.write(10);
///     writer.write(20);
///     writer.write(30);
///
///     // Read it back
///     assert_eq!(reader.read(), Some(10));
///     assert_eq!(reader.read(), Some(20));
///     assert_eq!(reader.read(), Some(30));
///     assert_eq!(reader.read(), None);
/// }
/// ```
pub struct QueueReader<T: Copy> {
    queue: *const RWRoundQueue<T>,
    _phantom: std::marker::PhantomData<T>,
}

unsafe impl<T: Copy + Send> Send for QueueReader<T> {}

impl<T: Copy> QueueReader<T> {
    /// Reads the next item from the queue.
    ///
    /// Returns `None` if the queue is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use os_lib::queue::RWRoundQueue;
    ///
    /// let mut queue = RWRoundQueue::<u32>::new(4).unwrap();
    ///
    /// unsafe {
    ///     let (reader, mut writer) = queue.split();
    ///
    ///     writer.write(42);
    ///     assert_eq!(reader.read(), Some(42));
    ///     assert_eq!(reader.read(), None);
    /// }
    /// ```
    pub fn read(&self) -> Option<T> {
        unsafe { (*self.queue).try_read() }
    }

    /// Reads up to `max` items from the queue in a single call.
    ///
    /// This is more efficient than calling `read()` multiple times when you
    /// need to process multiple items at once.
    ///
    /// # Arguments
    ///
    /// * `max` - Maximum number of items to read
    ///
    /// # Returns
    ///
    /// A vector containing all items read (may be less than `max` if queue
    /// doesn't have enough items).
    ///
    /// # Examples
    ///
    /// ```
    /// use os_lib::queue::RWRoundQueue;
    ///
    /// let mut queue = RWRoundQueue::<u32>::new(8).unwrap();
    ///
    /// unsafe {
    ///     let (reader, mut writer) = queue.split();
    ///
    ///     // Write 5 items
    ///     for i in 0..5 {
    ///         writer.write(i);
    ///     }
    ///
    ///     // Read up to 10 items (only 5 available)
    ///     let batch = reader.read_batch(10);
    ///     assert_eq!(batch, vec![0, 1, 2, 3, 4]);
    ///
    ///     // Queue is now empty
    ///     assert_eq!(reader.read_batch(5), vec![]);
    /// }
    /// ```
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

    /// Returns the approximate number of items available to read.
    ///
    /// See [`RWRoundQueue::len()`] for details about approximation in
    /// concurrent contexts.
    ///
    /// # Examples
    ///
    /// ```
    /// use os_lib::queue::RWRoundQueue;
    ///
    /// let mut queue = RWRoundQueue::<u32>::new(4).unwrap();
    ///
    /// unsafe {
    ///     let (reader, mut writer) = queue.split();
    ///
    ///     writer.write(1);
    ///     writer.write(2);
    ///     assert_eq!(reader.len(), 2);
    /// }
    /// ```
    pub fn len(&self) -> usize {
        unsafe { (*self.queue).len() }
    }

    /// Returns `true` if there are no items to read.
    ///
    /// # Examples
    ///
    /// ```
    /// use os_lib::queue::RWRoundQueue;
    ///
    /// let mut queue = RWRoundQueue::<u32>::new(4).unwrap();
    ///
    /// unsafe {
    ///     let (reader, mut writer) = queue.split();
    ///
    ///     assert!(reader.is_empty());
    ///     writer.write(1);
    ///     assert!(!reader.is_empty());
    /// }
    /// ```
    pub fn is_empty(&self) -> bool {
        unsafe { (*self.queue).is_empty() }
    }
}

/// Writer handle for lock-free queue access.
///
/// Created by calling [`RWRoundQueue::split()`]. This handle can be sent to
/// another thread to enable concurrent writing while a [`QueueReader`] reads
/// from a different thread.
///
/// # Thread Safety
///
/// `QueueWriter` implements `Send` (but not `Sync`), allowing it to be moved
/// to another thread. Only one writer should exist per queue.
///
/// # Examples
///
/// ```
/// use os_lib::queue::RWRoundQueue;
///
/// let mut queue = RWRoundQueue::<u32>::new(4).unwrap();
///
/// unsafe {
///     let (reader, mut writer) = queue.split();
///
///     // Write values
///     writer.write(100);
///     writer.write(200);
///
///     // Can be read by the reader
///     assert_eq!(reader.read(), Some(100));
///     assert_eq!(reader.read(), Some(200));
/// }
/// ```
pub struct QueueWriter<T: Copy> {
    queue: *mut RWRoundQueue<T>,
    _phantom: std::marker::PhantomData<T>,
}

unsafe impl<T: Copy + Send> Send for QueueWriter<T> {}

impl<T: Copy> QueueWriter<T> {
    /// Acquires a pointer to the next writable slot.
    ///
    /// This is a low-level API for zero-copy writes. Must be followed by
    /// a call to [`commit()`](Self::commit).
    ///
    /// # Returns
    ///
    /// Returns `Some((pointer, was_full))` where:
    /// - `pointer` - Raw pointer to write the value to
    /// - `was_full` - `true` if overwriting an old value
    ///
    /// # Safety
    ///
    /// Caller must:
    /// - Write exactly one value to the pointer
    /// - Call `commit(was_full)` after writing
    /// - Not use the pointer after committing
    ///
    /// # Examples
    ///
    /// ```
    /// use os_lib::queue::RWRoundQueue;
    /// use std::mem::MaybeUninit;
    /// use std::ptr;
    ///
    /// let mut queue = RWRoundQueue::<u32>::new(4).unwrap();
    ///
    /// unsafe {
    ///     let (reader, mut writer) = queue.split();
    ///
    ///     // Low-level write
    ///     if let Some((ptr, was_full)) = writer.acquire_ptr() {
    ///         ptr::write(ptr, MaybeUninit::new(999));
    ///         writer.commit(was_full);
    ///     }
    ///
    ///     assert_eq!(reader.read(), Some(999));
    /// }
    /// ```
    pub unsafe fn acquire_ptr(&mut self) -> Option<(*mut MaybeUninit<T>, bool)> {
        (*self.queue).acquire_write_ptr()
    }

    /// Commits a write operation after using [`acquire_ptr()`](Self::acquire_ptr).
    ///
    /// This advances the write pointer and, if the queue was full, also
    /// advances the read pointer to overwrite the oldest value.
    ///
    /// # Arguments
    ///
    /// * `was_full` - The boolean returned by `acquire_ptr()`
    ///
    /// # Safety
    ///
    /// Must be called exactly once after each successful `acquire_ptr()` call.
    /// The caller must have written a valid value to the pointer before calling.
    ///
    /// # Examples
    ///
    /// See [`acquire_ptr()`](Self::acquire_ptr) for usage example.
    pub unsafe fn commit(&mut self, was_full: bool) {
        (*self.queue).commit_write(was_full)
    }

    /// Writes a value to the queue, overwriting old data if full.
    ///
    /// This is a high-level convenience method that combines `acquire_ptr()`
    /// and `commit()` in a single call.
    ///
    /// # Arguments
    ///
    /// * `value` - The value to write
    ///
    /// # Returns
    ///
    /// Returns `true` if an old value was overwritten, `false` otherwise.
    ///
    /// # Safety
    ///
    /// Safe to call from writer thread. Caller must ensure proper
    /// synchronization in concurrent contexts.
    ///
    /// # Examples
    ///
    /// ```
    /// use os_lib::queue::RWRoundQueue;
    ///
    /// let mut queue = RWRoundQueue::<u32>::new(4).unwrap();
    ///
    /// unsafe {
    ///     let (reader, mut writer) = queue.split();
    ///
    ///     // Write values (capacity-1 slots are usable)
    ///     assert_eq!(writer.write(10), false);
    ///     assert_eq!(writer.write(20), false);
    ///     assert_eq!(writer.write(30), false);
    ///
    ///     // Next write overwrites oldest
    ///     assert_eq!(writer.write(40), true);
    ///
    ///     // First value is now 20, not 10
    ///     assert_eq!(reader.read(), Some(20));
    /// }
    /// ```
    pub unsafe fn write(&mut self, value: T) -> bool {
        (*self.queue).write_overwrite(value)
    }

    /// Returns the maximum capacity of the queue.
    ///
    /// # Examples
    ///
    /// ```
    /// use os_lib::queue::RWRoundQueue;
    ///
    /// let mut queue = RWRoundQueue::<u32>::new(8).unwrap();
    ///
    /// unsafe {
    ///     let (_reader, writer) = queue.split();
    ///     assert_eq!(writer.capacity(), 8);
    /// }
    /// ```
    pub fn capacity(&self) -> usize {
        unsafe { (*self.queue).capacity() }
    }
}

unsafe impl<T: Copy> Sync for QueueWriter<T> {} 
unsafe impl<T: Copy> Sync for QueueReader<T> {} 
