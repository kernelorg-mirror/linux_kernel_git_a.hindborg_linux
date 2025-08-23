// SPDX-License-Identifier: GPL-2.0

use super::{
    NullBlkDevice,
    THIS_MODULE, //
};
use kernel::{
    bindings,
    block::{
        badblocks::BadBlocks,
        mq::gen_disk::{
            GenDisk,
            GenDiskBuilder, //
        }, //
    },
    configfs::{
        self,
        AttributeOperations, //
    },
    configfs_attrs,
    fmt::{
        self,
        Write as _, //
    },
    new_mutex,
    page::PAGE_SIZE,
    prelude::*,
    str::{
        kstrtobool_bytes,
        CString, //
    },
    sync::{
        Arc,
        Mutex, //
    },
    time, //
};
use macros::{
    configfs_attribute,
    configfs_simple_bool_field,
    configfs_simple_field,
    show_field,
    store_number_with_power_check,
    store_with_power_check, //
};

mod macros;

pub(crate) fn subsystem() -> impl PinInit<kernel::configfs::Subsystem<Config>, Error> {
    let item_type = configfs_attrs! {
        container: configfs::Subsystem<Config>,
        data: Config,
        child: DeviceConfig,
        attributes: [
            features: 0,
        ],
    };

    kernel::configfs::Subsystem::new(c"rnull", item_type, try_pin_init!(Config {}))
}

#[pin_data]
pub(crate) struct Config {}

#[vtable]
impl AttributeOperations<0> for Config {
    type Data = Config;

    fn show(_this: &Config, page: &mut [u8; PAGE_SIZE]) -> Result<usize> {
        let mut writer = kernel::str::Formatter::new(page);
        writer.write_str(
            "blocksize,size,rotational,irqmode,completion_nsec,memory_backed,\
             submit_queues,use_per_node_hctx\n",
        )?;
        Ok(writer.bytes_written())
    }
}

#[vtable]
impl configfs::GroupOperations for Config {
    type Child = DeviceConfig;

    fn make_group(
        &self,
        name: &CStr,
    ) -> Result<impl PinInit<configfs::Group<DeviceConfig>, Error>> {
        let item_type = configfs_attrs! {
            container: configfs::Group<DeviceConfig>,
            data: DeviceConfig,
            attributes: [
                // Named for compatibility with C null_blk
                power: 0,
                blocksize: 1,
                rotational: 2,
                size: 3,
                irqmode: 4,
                completion_nsec: 5,
                memory_backed: 6,
                submit_queues: 7,
                use_per_node_hctx: 8,
                home_node: 9,
                discard: 10,
                no_sched:11,
                badblocks: 12,
            ],
        };

        Ok(configfs::Group::new(
            name.try_into()?,
            item_type,
            // TODO: cannot coerce new_mutex!() to impl PinInit<_, Error>, so put mutex inside
            try_pin_init!( DeviceConfig {
                data <- new_mutex!(DeviceConfigInner {
                    powered: false,
                    block_size: 4096,
                    rotational: false,
                    disk: None,
                    capacity_mib: 4096,
                    irq_mode: IRQMode::None,
                    completion_time: time::Delta::ZERO,
                    name: name.try_into()?,
                    memory_backed: false,
                    submit_queues: 1,
                    home_node: bindings::NUMA_NO_NODE,
                    discard: false,
                    no_sched: false,
                    bad_blocks: Arc::pin_init(BadBlocks::new(false), GFP_KERNEL)?,
                }),
            }),
            core::iter::empty(),
        ))
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum IRQMode {
    None,
    Soft,
    Timer,
}

impl TryFrom<u8> for IRQMode {
    type Error = kernel::error::Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            0 => Ok(Self::None),
            1 => Ok(Self::Soft),
            2 => Ok(Self::Timer),
            _ => Err(EINVAL),
        }
    }
}

impl fmt::Display for IRQMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("0")?,
            Self::Soft => f.write_str("1")?,
            Self::Timer => f.write_str("2")?,
        }
        Ok(())
    }
}

/// Wraps [`time::Delta`] to render the value as a bare nanosecond count for
/// configfs attributes that historically used this format.
struct DeltaDisplay(time::Delta);

impl kernel::fmt::Display for DeltaDisplay {
    fn fmt(&self, f: &mut kernel::fmt::Formatter<'_>) -> kernel::fmt::Result {
        f.write_fmt(kernel::prelude::fmt!("{}", self.0.as_nanos()))
    }
}

#[pin_data]
pub(crate) struct DeviceConfig {
    #[pin]
    data: Mutex<DeviceConfigInner>,
}

#[pin_data]
struct DeviceConfigInner {
    powered: bool,
    name: CString,
    block_size: u32,
    rotational: bool,
    capacity_mib: u64,
    irq_mode: IRQMode,
    completion_time: time::Delta,
    disk: Option<GenDisk<NullBlkDevice>>,
    memory_backed: bool,
    submit_queues: u32,
    home_node: i32,
    discard: bool,
    no_sched: bool,
    bad_blocks: Arc<BadBlocks>,
}

#[vtable]
impl configfs::AttributeOperations<0> for DeviceConfig {
    type Data = DeviceConfig;

    fn show(this: &DeviceConfig, page: &mut [u8; PAGE_SIZE]) -> Result<usize> {
        let mut writer = kernel::str::Formatter::new(page);

        if this.data.lock().powered {
            writer.write_str("1\n")?;
        } else {
            writer.write_str("0\n")?;
        }

        Ok(writer.bytes_written())
    }

    fn store(this: &DeviceConfig, page: &[u8]) -> Result {
        let power_op = kstrtobool_bytes(page)?;
        let mut guard = this.data.lock();

        if !guard.powered && power_op {
            guard.disk = Some(NullBlkDevice::new(crate::NullBlkOptions {
                name: &guard.name,
                block_size: guard.block_size,
                rotational: guard.rotational,
                capacity_mib: guard.capacity_mib,
                irq_mode: guard.irq_mode,
                completion_time: guard.completion_time,
                memory_backed: guard.memory_backed,
                submit_queues: guard.submit_queues,
                home_node: guard.home_node,
                discard: guard.discard,
                no_sched: guard.no_sched,
                bad_blocks: guard.bad_blocks.clone(),
            })?);
            guard.powered = true;
        } else if guard.powered && !power_op {
            drop(guard.disk.take());
            guard.powered = false;
        }

        Ok(())
    }
}

configfs_simple_field!(DeviceConfig, 1, block_size, u32, check GenDiskBuilder::validate_block_size);
configfs_simple_bool_field!(DeviceConfig, 2, rotational);
configfs_simple_field!(DeviceConfig, 3, capacity_mib, u64);
configfs_simple_field!(DeviceConfig, 4, irq_mode, IRQMode);
configfs_attribute!(DeviceConfig, 5,
    show: |this, page| show_field(DeltaDisplay(this.data.lock().completion_time), page),
    store: |this, page| store_number_with_power_check(this, page, |data, value: i64| {
        data.completion_time = time::Delta::from_nanos(value);
        Ok(())
    })
);

impl core::str::FromStr for IRQMode {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        let value: u8 = s.parse().map_err(|_| EINVAL)?;
        value.try_into()
    }
}

configfs_simple_bool_field!(DeviceConfig, 6, memory_backed);

#[vtable]
impl configfs::AttributeOperations<7> for DeviceConfig {
    type Data = DeviceConfig;

    fn show(this: &DeviceConfig, page: &mut [u8; PAGE_SIZE]) -> Result<usize> {
        let mut writer = kernel::str::Formatter::new(page);
        writer.write_fmt(fmt!("{}\n", this.data.lock().submit_queues))?;
        Ok(writer.bytes_written())
    }

    fn store(this: &DeviceConfig, page: &[u8]) -> Result {
        if this.data.lock().powered {
            return Err(EBUSY);
        }

        let text = core::str::from_utf8(page)?.trim();
        let value = text
            .parse::<u32>()
            .map_err(|_| kernel::error::code::EINVAL)?;

        if value == 0 || value > kernel::cpu::num_possible_cpus() {
            return Err(kernel::error::code::EINVAL);
        }

        this.data.lock().submit_queues = value;
        Ok(())
    }
}

configfs_attribute!(DeviceConfig, 8,
    show: |this, page| show_field(
        this.data.lock().submit_queues == kernel::numa::num_online_nodes(), page
    ),
    store: |this, page| store_with_power_check(this, page, |data, page| {
        let value = core::str::from_utf8(page)?
            .trim()
            .parse::<u8>()
            .map_err(|_| kernel::error::code::EINVAL)?
            != 0;

        if value {
            data.submit_queues = kernel::numa::num_online_nodes();
        }
        Ok(())
    })
);

configfs_simple_field!(
    DeviceConfig,
    9,
    home_node,
    i32,
    check(|value| {
        if value == 0 || value >= kernel::numa::num_online_nodes().try_into()? {
            Err(kernel::error::code::EINVAL)
        } else {
            Ok(())
        }
    })
);

configfs_attribute!(DeviceConfig, 10,
    show: |this, page| show_field(this.data.lock().discard, page),
    store: |this, page| store_with_power_check(this, page, |data, page| {
        if !data.memory_backed {
            return Err(EINVAL);
        }
        data.discard = kstrtobool_bytes(page)?;
        Ok(())
    })
);

configfs_simple_bool_field!(DeviceConfig, 11, no_sched);

#[vtable]
impl configfs::AttributeOperations<12> for DeviceConfig {
    type Data = DeviceConfig;

    fn show(this: &DeviceConfig, page: &mut [u8; PAGE_SIZE]) -> Result<usize> {
        let ret = this.data.lock().bad_blocks.show(page, false);
        if ret < 0 {
            Err(Error::from_errno(ret as c_int))
        } else {
            Ok(ret as usize)
        }
    }

    fn store(this: &DeviceConfig, page: &[u8]) -> Result {
        // This attribute can be set while device is powered.

        for line in core::str::from_utf8(page)?.lines() {
            let mut chars = line.chars();
            match chars.next() {
                Some(sign @ '+' | sign @ '-') => {
                    if let Some((start, end)) = chars.as_str().split_once('-') {
                        let start: u64 = start.parse().map_err(|_| EINVAL)?;
                        let end: u64 = end.parse().map_err(|_| EINVAL)?;

                        if start > end {
                            return Err(EINVAL);
                        }

                        this.data.lock().bad_blocks.enable();

                        if sign == '+' {
                            this.data.lock().bad_blocks.set_bad(start..=end, true)?;
                        } else {
                            this.data.lock().bad_blocks.set_good(start..=end)?;
                        }
                    }
                }
                _ => return Err(EINVAL),
            }
        }

        Ok(())
    }
}
