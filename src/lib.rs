#![no_std]

use core::{mem, ptr};
use libc::c_int;

/// Raw minimp3 bindings if you need them for whatever reason.
///
/// Although if there's a desired feature make an issue/PR.
pub mod ffi {
    #![allow(clippy::all, non_camel_case_types)]

    include!("bindings.rs");
}

/// Used to represent one PCM sample in output data (conditional).
///
/// Normally a signed 16-bit integer (i16), but if the "float" feature is enabled,
/// it's a 32-bit single precision float (f32).
#[cfg(not(feature = "float"))]
pub type Sample = i16;
#[cfg(feature = "float")]
pub type Sample = f32;

/// Streaming iterator yielding frame data & references to decoded PCM samples.
pub struct Decoder<'a> {
    data: &'a [u8],
    ffi_frame: ffi::mp3dec_frame_info_t,
    instance: ffi::mp3dec_t,
    pcm: [Sample; ffi::MINIMP3_MAX_SAMPLES_PER_FRAME as usize],

    // cache for peek/skip_frame, should be set to None upon any seeking otherwise it'll get stale
    last_frame_len: Option<usize>,
}

/// Info about the current frame yielded by a [Decoder](struct.Decoder.html).
#[derive(Debug)]
pub struct Frame<'a> {
    /// Bitrate of the source frame in kb/s.
    pub bitrate: u32,

    /// Number of channels in this frame.
    pub channels: u32,

    /// MPEG layer of this frame.
    pub mpeg_layer: u32,

    /// Reference to the samples in this frame, copy if needed to allocate.
    pub samples: &'a [Sample],

    /// Sample count per channel.
    /// Should be identical to `samples.len() / channels`
    /// unless you used [peek_frame](struct.Decoder.html#method.peek_frame).
    pub sample_count: u32,

    /// Sample rate of this frame in Hz.
    pub sample_rate: u32,

    /// Size of the source frame in bytes.
    pub source_len: usize,
}

impl<'a> Decoder<'a> {
    /// Creates a decoder over `data` (mp3 bytes).
    pub fn new(data: &'a (impl AsRef<[u8]> + ?Sized)) -> Self {
        Self {
            data: data.as_ref(),
            ffi_frame: unsafe { mem::zeroed() },
            instance: unsafe {
                let mut decoder: ffi::mp3dec_t = mem::zeroed();
                ffi::mp3dec_init(&mut decoder);
                decoder
            },
            pcm: [Default::default(); ffi::MINIMP3_MAX_SAMPLES_PER_FRAME as usize],
            last_frame_len: None,
        }
    }

    /// Reads the next frame, if available.
    /// If non-sample data (ex. ID3) is found it's skipped over until samples are found.
    pub fn next_frame(&mut self) -> Option<Frame> {
        self.last_frame_len = None;
        unsafe {
            let out_ptr: *mut Sample = self.pcm.as_mut_ptr();
            let samples = self.ffi_decode_frame(out_ptr) as u32;
            self.data = self
                .data
                .get_unchecked(self.ffi_frame.frame_bytes as usize..);
            if samples > 0 {
                Some(Frame {
                    bitrate: self.ffi_frame.bitrate_kbps as u32,
                    channels: self.ffi_frame.channels as u32,
                    samples: self
                        .pcm
                        .get_unchecked(..(samples * self.ffi_frame.channels as u32) as usize), // todo: feature?
                    sample_rate: self.ffi_frame.hz as u32,
                    mpeg_layer: self.ffi_frame.layer as u32,
                    sample_count: samples,
                    source_len: self.ffi_frame.frame_bytes as usize,
                })
            } else if self.ffi_frame.frame_bytes != 0 {
                self.next_frame()
            } else {
                None
            }
        }
    }

    /// Reads a frame without actually decoding it or advancing.
    /// Useful when you want to, for example, calculate the audio length.
    ///
    /// It should be noted that the [samples](struct.Frame.html#structfield.sample_count)
    /// in [Frame](struct.Frame.html) will be an empty slice since it's not decoding,
    /// but you can still read its [sample_count](struct.Frame.html#structfield.sample_count),
    /// which when zero will indicate that the current frame
    /// does not contain any samples to be decoded.
    /// Unlike [next_frame](struct.Frame.html#method.next_frame), it will **not** be skipped over
    /// automatically, but you can still of course call `skip_frame(frame.source_len)` on it.
    pub fn peek_frame(&mut self) -> Option<Frame> {
        let samples = unsafe { self.ffi_decode_frame(ptr::null_mut()) as u32 };
        if self.ffi_frame.frame_bytes != 0 {
            self.last_frame_len = Some(self.ffi_frame.frame_bytes as usize);
            Some(Frame {
                bitrate: self.ffi_frame.bitrate_kbps as u32,
                channels: self.ffi_frame.channels as u32,
                mpeg_layer: self.ffi_frame.layer as u32,
                samples: &[],
                sample_rate: self.ffi_frame.hz as u32,
                sample_count: samples,
                source_len: self.ffi_frame.frame_bytes as usize,
            })
        } else {
            None
        }
    }

    /// Skips ahead one frame.
    /// The frame won't be decoded, and if peek_frame was used previously it won't even be read again.
    pub fn skip_frame(&mut self) {
        if let Some(len) = self.frame_bytes() {
            unsafe { self.data = self.data.get_unchecked(len..) }
        }
    }

    fn frame_bytes(&mut self) -> Option<usize> {
        let len = self
            .last_frame_len
            .or_else(|| self.peek_frame().map(|f| f.source_len));
        self.last_frame_len = None;
        len
    }

    unsafe fn ffi_decode_frame(&mut self, pcm: *mut Sample) -> c_int {
        ffi::mp3dec_decode_frame(
            &mut self.instance,       // mp3dec instance
            self.data.as_ptr(),       // data pointer
            self.data.len() as c_int, // pointer length
            pcm,                      // output buffer
            &mut self.ffi_frame,      // frame info
        )
    }
}
