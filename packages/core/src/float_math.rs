#[inline]
pub(crate) fn trunc(x: f64) -> f64 {
    #[cfg(feature = "std")]
    {
        x.trunc()
    }
    #[cfg(not(feature = "std"))]
    {
        if x >= 0.0 {
            (x as i64) as f64
        } else {
            -((-x) as i64) as f64
        }
    }
}

#[inline]
pub(crate) fn floor(x: f64) -> f64 {
    #[cfg(feature = "std")]
    {
        x.floor()
    }
    #[cfg(not(feature = "std"))]
    {
        let i = x as i64;
        let fi = i as f64;
        if x < fi {
            fi - 1.0
        } else {
            fi
        }
    }
}
