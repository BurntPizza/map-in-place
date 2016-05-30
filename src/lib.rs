
pub trait MapInPlace<A, B>: Sized {
    /// Should be of the same base type as the implementor.  
    /// E.g. `Vec<B>` when implementing for `Vec<A>`
    type Output;

    /// Apply a mapping function to `self` without allocating. 
    /// Makes best effort to maintain the invariant  
    ///  
    /// `self.as_ptr() as *const () == self.map_in_place(..).as_ptr() as *const ()`  
    ///  
    /// An example of a case where this isn't possible is for Vec where B is zero-sized but A is not.
    fn map_in_place<F>(self, f: F) -> Self::Output where F: FnMut(A) -> B;
}

impl<A, B> MapInPlace<A, B> for Vec<A> {
    type Output = Vec<B>;

    #[inline]
    fn map_in_place<F>(mut self, mut f: F) -> Self::Output
        where F: FnMut(A) -> B
    {
        use std::mem;
        use std::ptr;
        use std::cmp::{Ord, Ordering};

        macro_rules! map_loop {
            () => {{
                let ptr_a = self.as_ptr();
                let ptr_b = ptr_a as *mut B;
                let len = self.len();

                for i in 0..len {
                    unsafe {
                        let ptr_a = ptr_a.offset(i as isize);
                        let ptr_b = ptr_b.offset(i as isize);

                        ptr::write(ptr_b, f(ptr::read(ptr_a)));
                    }
                }

                (ptr_b, len)
            }}
        }

        let a_size = mem::size_of::<A>();
        let b_size = mem::size_of::<B>();

        match a_size.cmp(&b_size) {
            Ordering::Equal => {
                // map_loop!();
                self.as_mut_slice().map_in_place(f);

                unsafe { mem::transmute(self) }
            }
            Ordering::Greater => {
                if b_size == 0 {
                    // doesn't preserve address invariant
                    let mut v = Vec::with_capacity(0);

                    for e in self.into_iter() {
                        v.push(f(e));
                    }

                    v
                } else {
                    let cap = self.capacity().checked_mul(a_size).map(|x| x / b_size).unwrap();
                    let (ptr_b, len) = map_loop!();

                    mem::forget(self);

                    unsafe { Vec::from_raw_parts(ptr_b, len, cap) }
                }
            }
            Ordering::Less => panic!(),
        }
    }
}

impl<'a, A, B: 'a> MapInPlace<A, B> for &'a mut [A] {
    type Output = &'a mut [B];

    #[inline]
    fn map_in_place<F>(self, mut f: F) -> Self::Output
        where F: FnMut(A) -> B
    {
        use std::mem;
        use std::ptr;
        use std::cmp::{Ord, Ordering};

        let a_size = mem::size_of::<A>();
        let b_size = mem::size_of::<B>();

        match a_size.cmp(&b_size) {
            Ordering::Equal => {
                let ptr_a = self.as_ptr();
                let ptr_b = ptr_a as *mut B;

                for i in 0..self.len() {
                    unsafe {
                        let ptr_a = ptr_a.offset(i as isize);
                        let ptr_b = ptr_b.offset(i as isize);

                        ptr::write(ptr_b, f(ptr::read(ptr_a)));
                    }
                }

                unsafe { mem::transmute(self) }
            }
            Ordering::Greater => panic!(),
            Ordering::Less => panic!(),
        }
    }
}

#[cfg(test)]
mod tests {

    use super::MapInPlace;

    #[test]
    fn same_size_vec() {
        let v = vec![0, 1, 2, 3];

        let bp = v.as_ptr() as *const ();
        let v = v.map_in_place(|x: u32| (x * x) as i32);
        let ap = v.as_ptr() as *const ();

        assert_eq!(bp, ap); // still at same memory addr
        assert_eq!(v, vec![0, 1, 4, 9]);
    }

    #[test]
    fn different_sizes_vec() {
        let v = vec![0, 1, 2, 3];

        let bp = v.as_ptr() as *const ();
        let v = v.map_in_place(|x: u32| (x * x) as i16);
        let ap = v.as_ptr() as *const ();

        assert_eq!(bp, ap); // still at same memory addr
        assert_eq!(v, vec![0, 1, 4, 9]);
    }

    #[test]
    fn both_zst_vec() {
        #[derive(Debug)]
        struct Zst;

        let v = vec![(), (), (), ()];

        let bp = v.as_ptr() as *const ();
        let v = v.map_in_place(|_| Zst);
        let ap = v.as_ptr() as *const ();

        assert_eq!(bp, ap); // still at same memory addr
    }

    #[test]
    fn nzst_to_zst_vec() {
        let v = vec![0, 1, 2, 3];

        let bp = v.as_ptr() as *const ();
        let v = v.map_in_place(|_: u32| ());
        let ap = v.as_ptr() as *const ();

        assert!(bp != ap); // -- NOT -- still at same memory addr
    }

    #[test]
    #[should_panic]
    fn zst_to_nzst_vec() {
        let v = vec![(), (), (), ()];

        let bp = v.as_ptr() as *const ();
        let v = v.map_in_place(|_| 0usize);
        let ap = v.as_ptr() as *const ();

        assert_eq!(bp, ap); // still at same memory addr
    }

    // //////

    #[test]
    fn same_size_slice() {
        let mut v = vec![0, 1, 2, 3];

        let bp = v.as_ptr() as *const ();
        let v = (&mut *v).map_in_place(|x: u32| (x * x) as i32);
        let ap = v.as_ptr() as *const ();

        assert_eq!(bp, ap); // still at same memory addr
        assert_eq!(v, &*vec![0, 1, 4, 9]);
    }

    #[test]
    #[should_panic]
    fn different_sizes_slice() {
        let mut v = vec![0, 1, 2, 3];

        let bp = v.as_ptr() as *const ();
        let v = (&mut *v).map_in_place(|x: u32| (x * x) as i16);
        let ap = v.as_ptr() as *const ();

        assert_eq!(bp, ap); // still at same memory addr
        assert_eq!(v, &*vec![0, 1, 4, 9]);
    }

    #[test]
    fn both_zst_slice() {
        #[derive(Debug)]
        struct Zst;

        let mut v = vec![(), (), (), ()];

        let bp = v.as_ptr() as *const ();
        let v = (&mut *v).map_in_place(|_| Zst);
        let ap = v.as_ptr() as *const ();

        assert_eq!(bp, ap); // still at same memory addr
    }

    #[test]
    #[should_panic]
    fn nzst_to_zst_slice() {
        let mut v = vec![0, 1, 2, 3];

        let bp = v.as_ptr() as *const ();
        let v = (&mut *v).map_in_place(|_: u32| ());
        let ap = v.as_ptr() as *const ();

        assert!(bp != ap); // -- NOT -- still at same memory addr
    }

    #[test]
    #[should_panic]
    fn zst_to_nzst_slice() {
        let mut v = vec![(), (), (), ()];

        let bp = v.as_ptr() as *const ();
        let v = (&mut *v).map_in_place(|_| 0usize);
        let ap = v.as_ptr() as *const ();

        assert_eq!(bp, ap); // still at same memory addr
    }
}
