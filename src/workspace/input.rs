pub(super) fn parse_frame_pair(value: &str) -> Result<(u64, u64), String> {
    let (start, end) = value
        .split_once(':')
        .ok_or_else(|| "Source region must use START:END".to_owned())?;
    let start = start
        .trim()
        .parse::<u64>()
        .map_err(|_| "Source-region start must be an unsigned frame".to_owned())?;
    let end = end
        .trim()
        .parse::<u64>()
        .map_err(|_| "Source-region end must be an unsigned frame".to_owned())?;
    Ok((start, end))
}

pub(super) fn parse_fade_pair(value: &str) -> Result<(u32, u32), String> {
    let (fade_in, fade_out) = value
        .split_once(':')
        .ok_or_else(|| "Source fades must use IN:OUT".to_owned())?;
    let fade_in = fade_in
        .trim()
        .parse::<u32>()
        .map_err(|_| "Fade-in must be an unsigned frame count".to_owned())?;
    let fade_out = fade_out
        .trim()
        .parse::<u32>()
        .map_err(|_| "Fade-out must be an unsigned frame count".to_owned())?;
    Ok((fade_in, fade_out))
}
