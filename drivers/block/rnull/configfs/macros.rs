// SPDX-License-Identifier: GPL-2.0

use super::{
    DeviceConfig,
    DeviceConfigInner, //
};
use core::str::FromStr;
use kernel::{
    fmt::{
        self,
        Write, //
    },
    page::PAGE_SIZE,
    prelude::*,
};

pub(crate) fn show_field<T: fmt::Display>(value: T, page: &mut [u8; PAGE_SIZE]) -> Result<usize> {
    let mut writer = kernel::str::Formatter::new(page);
    writer.write_fmt(fmt!("{}\n", value))?;
    Ok(writer.bytes_written())
}

// The lock guard is passed to `store_fn` so the powered check and the
// store happen atomically. Releasing the lock between the two would
// allow another writer to power the device on in the gap.
pub(crate) fn store_with_power_check<F>(this: &DeviceConfig, page: &[u8], store_fn: F) -> Result
where
    F: FnOnce(&mut DeviceConfigInner, &[u8]) -> Result,
{
    let mut guard = this.data.lock();
    if guard.powered {
        return Err(EBUSY);
    }
    store_fn(&mut guard, page)
}

pub(crate) fn store_number_with_power_check<F, T>(
    this: &DeviceConfig,
    page: &[u8],
    store_fn: F,
) -> Result
where
    F: FnOnce(&mut DeviceConfigInner, T) -> Result,
    T: FromStr,
{
    let text = core::str::from_utf8(page)?.trim();
    let value = text.parse::<T>().map_err(|_| EINVAL)?;

    let mut guard = this.data.lock();
    if guard.powered {
        return Err(EBUSY);
    }

    store_fn(&mut guard, value)
}

macro_rules! configfs_attribute {
    (
        $type:ty,
        $id:literal,
        show: |$show_this:ident, $show_page:ident| $show_block:expr,
        store: |$store_this:ident, $store_page:ident| $store_block:expr
        $(,)?
    ) => {
        #[vtable]
        impl configfs::AttributeOperations<$id> for $type {
            type Data = $type;

            fn show($show_this: &$type, $show_page: &mut [u8; PAGE_SIZE]) -> Result<usize> {
                $show_block
            }

            fn store($store_this: &$type, $store_page: &[u8]) -> Result {
                $store_block
            }
        }
    };
}
pub(crate) use configfs_attribute;

// Specialized macro for simple boolean fields that just store kstrtobool_bytes result.
macro_rules! configfs_simple_bool_field {
    ($type:ty, $id:literal, $field:ident) => {
        crate::configfs::macros::configfs_attribute!($type, $id,
            show: |this, page| crate::configfs::macros::show_field(this.data.lock().$field, page),
            store: |this, page|
              crate::configfs::macros::store_with_power_check(this, page, |data, page| {
                data.$field = kstrtobool_bytes(page)?;
                Ok(())
            })
        );
    };
}
pub(crate) use configfs_simple_bool_field;

// Specialized macro for simple numeric fields that just parse and assign
macro_rules! configfs_simple_field {
    // Simple direct assignment
    ($type:ty, $id:literal, $field:ident, $field_type:ty) => {
        crate::configfs::macros::configfs_attribute!($type, $id,
            show: |this, page| crate::configfs::macros::show_field(this.data.lock().$field, page),
            store: |this, page| crate::configfs::macros::store_number_with_power_check(
                this,
                page,
                |data, value: $field_type| {
                    data.$field = value;
                    Ok(())
                }
            )
        );
    };
    // With infallible conversion expression (direct value)
    ($type:ty, $id:literal, $field:ident, $field_type:ty, into $convert:expr) => {
        crate::configfs::macros::configfs_attribute!($type, $id,
            show: |this, page|
                crate::configfs::macros::show_field(this.data.lock().$field, page),
            store: |this, page| crate::configfs::macros::store_number_with_power_check(
                this,
                page,
                |data, value: $field_type| {
                    data.$field = $convert(value);
                    Ok(())
                }
            )
        );
    };
    // With check, no conversion
    ($type:ty, $id:literal, $field:ident, $field_type:ty, check $check:expr) => {
        crate::configfs::macros::configfs_attribute!($type, $id,
            show: |this, page| crate::configfs::macros::show_field(this.data.lock().$field, page),
            store: |this, page| crate::configfs::macros::store_number_with_power_check(
                this,
                page,
                |data, value: $field_type| {
                    $check(value)?;
                    data.$field = value;
                    Ok(())
                }
            )
        );
    };
}
pub(crate) use configfs_simple_field;
