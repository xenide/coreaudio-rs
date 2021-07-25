//! This module is an attempt to provide a friendly, rust-esque interface to Apple's Audio Unit API.
//!
//! Learn more about the Audio Unit API [here](https://developer.apple.com/library/mac/documentation/MusicAudio/Conceptual/AudioUnitProgrammingGuide/Introduction/Introduction.html#//apple_ref/doc/uid/TP40003278-CH1-SW2)
//! and [here](https://developer.apple.com/library/mac/documentation/MusicAudio/Conceptual/AudioUnitProgrammingGuide/TheAudioUnit/TheAudioUnit.html).
//!
//! TODO: The following are `kAudioUnitSubType`s (along with their const u32) generated by
//! rust-bindgen that we could not find any documentation on:
//!
//! - MIDISynth            = 1836284270,
//! - RoundTripAAC         = 1918984547,
//! - SpatialMixer         = 862217581,
//! - SphericalHeadPanner  = 1936746610,
//! - VectorPanner         = 1986158963,
//! - SoundFieldPanner     = 1634558569,
//! - HRTFPanner           = 1752331366,
//! - NetReceive           = 1852990326,
//!
//! If you can find documentation on these, please feel free to submit an issue or PR with the
//! fixes!


use crate::error::Error;
use std::mem;
use std::ptr;
use std::os::raw::{c_uint, c_void};
use sys;

pub use self::audio_format::AudioFormat;
pub use self::sample_format::{SampleFormat, Sample};
pub use self::stream_format::StreamFormat;
pub use self::types::{
    Type,
    EffectType,
    FormatConverterType,
    GeneratorType,
    IOType,
    MixerType,
    MusicDeviceType,
};


pub mod audio_format;
pub mod render_callback;
pub mod sample_format;
pub mod stream_format;
pub mod types;


/// The input and output **Scope**s.
///
/// More info [here](https://developer.apple.com/library/ios/documentation/AudioUnit/Reference/AudioUnitPropertiesReference/index.html#//apple_ref/doc/constant_group/Audio_Unit_Scopes)
/// and [here](https://developer.apple.com/library/mac/documentation/MusicAudio/Conceptual/AudioUnitProgrammingGuide/TheAudioUnit/TheAudioUnit.html).
#[derive(Copy, Clone, Debug)]
pub enum Scope {
    Global = 0,
    Input  = 1,
    Output = 2,
    Group = 3,
    Part = 4,
    Note = 5,
    Layer = 6,
    LayerItem = 7,
}

/// Represents the **Input** and **Output** **Element**s.
///
/// These are used when specifying which **Element** we're setting the properties of.
#[derive(Copy, Clone, Debug)]
pub enum Element {
    Output = 0,
    Input  = 1,
}


/// A rust representation of the sys::AudioUnit, including a pointer to the current rendering callback.
///
/// Find the original Audio Unit Programming Guide [here](https://developer.apple.com/library/mac/documentation/MusicAudio/Conceptual/AudioUnitProgrammingGuide/TheAudioUnit/TheAudioUnit.html).
pub struct AudioUnit {
    instance: sys::AudioUnit,
    maybe_render_callback: Option<*mut render_callback::InputProcFnWrapper>,
    maybe_input_callback: Option<InputCallback>,
}

struct InputCallback {
    // The audio buffer list to which input data is rendered.
    buffer_list: *mut sys::AudioBufferList,
    callback: *mut render_callback::InputProcFnWrapper,
}


macro_rules! try_os_status {
    ($expr:expr) => (Error::from_os_status($expr)?)
}


impl AudioUnit {

    /// Construct a new AudioUnit with any type that may be automatically converted into
    /// [**Type**](./enum.Type).
    ///
    /// Here is a list of compatible types:
    ///
    /// - [**Type**](./types/enum.Type)
    /// - [**IOType**](./types/enum.IOType)
    /// - [**MusicDeviceType**](./types/enum.MusicDeviceType)
    /// - [**GeneratorType**](./types/enum.GeneratorType)
    /// - [**FormatConverterType**](./types/enum.FormatConverterType)
    /// - [**EffectType**](./types/enum.EffectType)
    /// - [**MixerType**](./types/enum.MixerType)
    ///
    /// To construct the **AudioUnit** with some component flags, see
    /// [**AudioUnit::new_with_flags**](./struct.AudioUnit#method.new_with_flags).
    ///
    /// Note: the `AudioUnit` is constructed with the `kAudioUnitManufacturer_Apple` Manufacturer
    /// Identifier, as this is the only Audio Unit Manufacturer Identifier documented by Apple in
    /// the AudioUnit reference (see [here](https://developer.apple.com/library/prerelease/mac/documentation/AudioUnit/Reference/AUComponentServicesReference/index.html#//apple_ref/doc/constant_group/Audio_Unit_Manufacturer_Identifier)).
    pub fn new<T>(ty: T) -> Result<AudioUnit, Error>
        where T: Into<Type>,
    {
        AudioUnit::new_with_flags(ty, 0, 0)
    }

    /// The same as [**AudioUnit::new**](./struct.AudioUnit#method.new) but with the given
    /// component flags and mask.
    pub fn new_with_flags<T>(ty: T, flags: u32, mask: u32) -> Result<AudioUnit, Error>
        where T: Into<Type>,
    {
        const MANUFACTURER_IDENTIFIER: u32 = sys::kAudioUnitManufacturer_Apple;
        let au_type: Type = ty.into();
        let sub_type_u32 = match au_type.as_subtype_u32() {
            Some(u) => u,
            None => return Err(Error::NoKnownSubtype),
        };

        // A description of the audio unit we desire.
        let desc = sys::AudioComponentDescription {
            componentType: au_type.as_u32() as c_uint,
            componentSubType: sub_type_u32 as c_uint,
            componentManufacturer: MANUFACTURER_IDENTIFIER,
            componentFlags: flags,
            componentFlagsMask: mask,
        };

        unsafe {
            // Find the default audio unit for the description.
            //
            // From the "Audio Unit Hosting Guide for iOS":
            //
            // Passing NULL to the first parameter of AudioComponentFindNext tells this function to
            // find the first system audio unit matching the description, using a system-defined
            // ordering. If you instead pass a previously found audio unit reference in this
            // parameter, the function locates the next audio unit matching the description.
            let component = sys::AudioComponentFindNext(ptr::null_mut(), &desc as *const _);
            if component.is_null() {
                return Err(Error::NoMatchingDefaultAudioUnitFound);
            }

            // Create an instance of the default audio unit using the component.
            let mut instance_uninit = mem::MaybeUninit::<sys::AudioUnit>::uninit();
            try_os_status!(
                sys::AudioComponentInstanceNew(component, instance_uninit.as_mut_ptr() as *mut sys::AudioUnit)
            );
            let instance: sys::AudioUnit = instance_uninit.assume_init();

            // Initialise the audio unit!
            try_os_status!(sys::AudioUnitInitialize(instance));
            Ok(AudioUnit {
                instance,
                maybe_render_callback: None,
                maybe_input_callback: None,
            })
        }
    }

    /// On successful initialization, the audio formats for input and output are valid
    /// and the audio unit is ready to render. During initialization, an audio unit
    /// allocates memory according to the maximum number of audio frames it can produce
    /// in response to a single render call.
    ///
    /// Usually, the state of an audio unit (such as its I/O formats and memory allocations)
    /// cannot be changed while an audio unit is initialized.
    pub fn initialize(&mut self) -> Result<(), Error> {
        unsafe { try_os_status!(sys::AudioUnitInitialize(self.instance)); }
        Ok(())
    }

    /// Before you change an initialize audio unit’s processing characteristics,
    /// such as its input or output audio data format or its sample rate, you must
    /// first uninitialize it. Calling this function deallocates the audio unit’s resources.
    ///
    /// After calling this function, you can reconfigure the audio unit and then call
    /// AudioUnitInitialize to reinitialize it.
    pub fn uninitialize(&mut self) -> Result<(), Error> {
        unsafe { try_os_status!(sys::AudioUnitUninitialize(self.instance)); }
        Ok(())
    }

    /// Sets the value for some property of the **AudioUnit**.
    ///
    /// To clear an audio unit property value, set the data paramater with `None::<()>`.
    ///
    /// Clearing properties only works for those properties that do not have a default value.
    ///
    /// For more on "properties" see [the reference](https://developer.apple.com/library/ios/documentation/AudioUnit/Reference/AudioUnitPropertiesReference/index.html#//apple_ref/doc/uid/TP40007288).
    ///
    /// **Available** in iOS 2.0 and later.
    ///
    /// Parameters
    /// ----------
    ///
    /// - **id**: The identifier of the property.
    /// - **scope**: The audio unit scope for the property.
    /// - **elem**: The audio unit element for the property.
    /// - **maybe_data**: The value that you want to apply to the property.
    pub fn set_property<T>(&mut self, id: u32, scope: Scope, elem: Element, maybe_data: Option<&T>)
        -> Result<(), Error>
    {
        set_property(self.instance, id, scope, elem, maybe_data)
    }

    /// Gets the value of an **AudioUnit** property.
    ///
    /// **Available** in iOS 2.0 and later.
    ///
    /// Parameters
    /// ----------
    ///
    /// - **id**: The identifier of the property.
    /// - **scope**: The audio unit scope for the property.
    /// - **elem**: The audio unit element for the property.
    pub fn get_property<T>(&self, id: u32, scope: Scope, elem: Element) -> Result<T, Error> {
        get_property(self.instance, id, scope, elem)
    }

    /// Starts an I/O **AudioUnit**, which in turn starts the audio unit processing graph that it is
    /// connected to.
    ///
    /// **Available** in OS X v10.0 and later.
    pub fn start(&mut self) -> Result<(), Error> {
        unsafe { try_os_status!(sys::AudioOutputUnitStart(self.instance)); }
        Ok(())
    }

    /// Stops an I/O **AudioUnit**, which in turn stops the audio unit processing graph that it is
    /// connected to.
    ///
    /// **Available** in OS X v10.0 and later.
    pub fn stop(&mut self) -> Result<(), Error> {
        unsafe { try_os_status!(sys::AudioOutputUnitStop(self.instance)); }
        Ok(())
    }

    /// Set the **AudioUnit**'s sample rate.
    ///
    /// **Available** in iOS 2.0 and later.
    pub fn set_sample_rate(&mut self, sample_rate: f64) -> Result<(), Error> {
        let id = sys::kAudioUnitProperty_SampleRate;
        self.set_property(id, Scope::Input, Element::Output, Some(&sample_rate))
    }

    /// Get the **AudioUnit**'s sample rate.
    pub fn sample_rate(&self) -> Result<f64, Error> {
        let id = sys::kAudioUnitProperty_SampleRate;
        self.get_property(id, Scope::Input, Element::Output)
    }

    /// Sets the current **StreamFormat** for the AudioUnit.
    ///
    /// Core Audio uses slightly different defaults depending on the platform.
    ///
    /// From the Core Audio Overview:
    ///
    /// > The canonical formats in Core Audio are as follows:
    /// >
    /// > - iOS input and output: Linear PCM with 16-bit integer samples.
    /// > - iOS audio units and other audio processing: Noninterleaved linear PCM with 8.24-bit
    /// fixed-point samples
    /// > - Mac input and output: Linear PCM with 32-bit floating point samples.
    /// > - Mac audio units and other audio processing: Noninterleaved linear PCM with 32-bit
    /// floating-point
    pub fn set_stream_format(
        &mut self,
        stream_format: StreamFormat,
        scope: Scope,
    ) -> Result<(), Error> {
        let id = sys::kAudioUnitProperty_StreamFormat;
        let asbd = stream_format.to_asbd();
        self.set_property(id, scope, Element::Output, Some(&asbd))
    }

    /// Return the current Stream Format for the AudioUnit.
    pub fn stream_format(&self, scope: Scope) -> Result<StreamFormat, Error> {
        let id = sys::kAudioUnitProperty_StreamFormat;
        let asbd = self.get_property(id, scope, Element::Output)?;
        StreamFormat::from_asbd(asbd)
    }

    /// Return the current output Stream Format for the AudioUnit.
    pub fn output_stream_format(&self) -> Result<StreamFormat, Error> {
        self.stream_format(Scope::Output)
    }

    /// Return the current input Stream Format for the AudioUnit.
    pub fn input_stream_format(&self) -> Result<StreamFormat, Error> {
        self.stream_format(Scope::Input)
    }
}


unsafe impl Send for AudioUnit {}


impl Drop for AudioUnit {
    fn drop(&mut self) {
        unsafe {
            use crate::error;

            // We don't want to panic in `drop`, so we'll ignore returned errors.
            //
            // A user should explicitly terminate the `AudioUnit` if they want to handle errors (we
            // still need to provide a way to actually do that).
            self.stop().ok();
            error::Error::from_os_status(sys::AudioUnitUninitialize(self.instance)).ok();

            self.free_render_callback();
            self.free_input_callback();

            error::Error::from_os_status(sys::AudioComponentInstanceDispose(self.instance)).ok();
        }
    }
}


/// Sets the value for some property of the **AudioUnit**.
///
/// To clear an audio unit property value, set the data paramater with `None::<()>`.
///
/// Clearing properties only works for those properties that do not have a default value.
///
/// For more on "properties" see [the reference](https://developer.apple.com/library/ios/documentation/AudioUnit/Reference/AudioUnitPropertiesReference/index.html#//apple_ref/doc/uid/TP40007288).
///
/// **Available** in iOS 2.0 and later.
///
/// Parameters
/// ----------
///
/// - **au**: The AudioUnit instance.
/// - **id**: The identifier of the property.
/// - **scope**: The audio unit scope for the property.
/// - **elem**: The audio unit element for the property.
/// - **maybe_data**: The value that you want to apply to the property.
pub fn set_property<T>(
    au: sys::AudioUnit,
    id: u32,
    scope: Scope,
    elem: Element,
    maybe_data: Option<&T>,
) -> Result<(), Error>
{
    let (data_ptr, size) = maybe_data.map(|data| {
        let ptr = data as *const _ as *const c_void;
        let size = ::std::mem::size_of::<T>() as u32;
        (ptr, size)
    }).unwrap_or_else(|| (::std::ptr::null(), 0));
    let scope = scope as c_uint;
    let elem = elem as c_uint;
    unsafe {
        try_os_status!(sys::AudioUnitSetProperty(au, id, scope, elem, data_ptr, size))
    }
    Ok(())
}

/// Gets the value of an **AudioUnit** property.
///
/// **Available** in iOS 2.0 and later.
///
/// Parameters
/// ----------
///
/// - **au**: The AudioUnit instance.
/// - **id**: The identifier of the property.
/// - **scope**: The audio unit scope for the property.
/// - **elem**: The audio unit element for the property.
pub fn get_property<T>(
    au: sys::AudioUnit,
    id: u32,
    scope: Scope,
    elem: Element,
) -> Result<T, Error>
{
    let scope = scope as c_uint;
    let elem = elem as c_uint;
    let mut size = ::std::mem::size_of::<T>() as u32;
    unsafe {
        let mut data_uninit = ::std::mem::MaybeUninit::<T>::uninit();
        let data_ptr = data_uninit.as_mut_ptr() as *mut _ as *mut c_void;
        let size_ptr = &mut size as *mut _;
        try_os_status!(
            sys::AudioUnitGetProperty(au, id, scope, elem, data_ptr, size_ptr)
        );
        let data: T = data_uninit.assume_init();
        Ok(data)
    }
}

/// Gets the value of a specified audio session property.
///
/// **Available** in iOS 2.0 and later.
///
/// Parameters
/// ----------
///
/// - **id**: The identifier of the property.
#[cfg(target_os = "ios")]
pub fn audio_session_get_property<T>(
    id: u32,
) -> Result<T, Error>
{
    let mut size = ::std::mem::size_of::<T>() as u32;
    unsafe {
        let mut data: T = ::std::mem::uninitialized();
        let data_ptr = &mut data as *mut _ as *mut c_void;
        let size_ptr = &mut size as *mut _;
        try_os_status!(
            sys::AudioSessionGetProperty(id, size_ptr, data_ptr)
        );
        Ok(data)
    }
}
