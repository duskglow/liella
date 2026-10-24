//! The typed `va_list` reader (IP-14, finding F11): a [`VaSource`] yields the next argument of
//! a C `va_list` by its register class. The boundary implements it with the C layer's `va_arg`
//! accessors; the formatted-I/O engines read arguments only through it.

/// The arguments of a C `va_list`, in order.
pub trait VaSource {
    /// The next integer or pointer argument (`va_arg(ap, long)`): the full register slot.
    fn int(&mut self) -> u64;
    /// The next `double` (`va_arg(ap, double)`), as bits.
    fn double(&mut self) -> u64;
    /// The next `long double` (`va_arg(ap, long double)`): the 80 bits of the x87 value.
    fn long_double(&mut self) -> u128;
}
