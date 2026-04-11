use caps::{CapSet, Capability};

pub fn drop_privileges() -> Result<(), String> {
    let cap = Capability::CAP_DAC_OVERRIDE;

    let has_effective = caps::has_cap(None, CapSet::Effective, cap).map_err(|e| e.to_string())?;
    if has_effective {
        caps::drop(None, CapSet::Effective, cap).map_err(|e| e.to_string())?;
    }

    let has_permitted = caps::has_cap(None, CapSet::Permitted, cap).map_err(|e| e.to_string())?;
    if has_permitted {
        caps::drop(None, CapSet::Permitted, cap).map_err(|e| e.to_string())?;
    }

    let has_inheritable =
        caps::has_cap(None, CapSet::Inheritable, cap).map_err(|e| e.to_string())?;
    if has_inheritable {
        caps::drop(None, CapSet::Inheritable, cap).map_err(|e| e.to_string())?;
    }

    Ok(())
}
