/// Defines a struct whose fields are written one after the other, in the order listed. The fields are listed once,
/// for the struct and for both directions. Without fields it is a unit struct (`Armed`, not `Armed {}`).
macro_rules! wire_struct {
    ($(#[$meta:meta])* $name:ident {}) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub struct $name;

        impl $crate::wire::payload::Wire for $name {
            fn read(_: &mut $crate::wire::payload::Reader) -> Result<Self, $crate::wire::message::MessageError> {
                return Ok(Self);
            }

            fn write(&self, _: &mut $crate::wire::payload::Writer) {}
        }
    };
    ($(#[$meta:meta])* $name:ident { $($(#[$field_meta:meta])* $field:ident: $ty:ty),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub struct $name {
            $($(#[$field_meta])* pub $field: $ty,)+
        }

        impl $crate::wire::payload::Wire for $name {
            fn read(r: &mut $crate::wire::payload::Reader) -> Result<Self, $crate::wire::message::MessageError> {
                return Ok(Self {
                    $($field: <$ty as $crate::wire::payload::Wire>::read(r)?,)+
                });
            }

            fn write(&self, w: &mut $crate::wire::payload::Writer) {
                $($crate::wire::payload::Wire::write(&self.$field, w);)+
            }
        }
    };
}
pub(crate) use wire_struct;
