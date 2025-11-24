use std::marker::PhantomData;

use uom::si;

mod sealed {
    pub trait Sealed {}
}

pub trait Quantity {
    type Value;

    fn test() -> &'static str {
        return "This is a Quantity!";
    }
}

impl<D, U, V> Quantity for si::Quantity<D, U, V>
where
    D: si::Dimension + ?Sized,
    U: si::Units<V> + ?Sized,
    V: uom::num::Num + uom::Conversion<V> + PartialOrd,
{
    type Value = V;
}

pub struct Unquantified<Value> {
    value: PhantomData<Value>,
}

impl<V> Quantity for Unquantified<V> {
    type Value = V;
}

pub trait Message: sealed::Sealed {}
//pub trait FieldOf<M: Message + ?Sized> {}

pub trait Metric: sealed::Sealed {
    type Quantity: Quantity;
    // fn metric() -> telemetry::Metric;
    fn name() -> &'static str; //Test Only
}

macros::generate_metrics!();
