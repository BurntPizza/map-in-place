
#[cfg(test)]
#[macro_use]
extern crate lazy_static;

use std::marker::PhantomData;
use std::ptr;
use std::mem;

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
    fn map_in_place<F>(self, mut f: F) -> Self::Output
        where F: FnMut(A) -> B
    {
        let a_size = mem::size_of::<A>();
        let b_size = mem::size_of::<B>();
        let ptr_a = self.as_ptr();
        let ptr_b = ptr_a as *mut B;
        let len = self.len();

        if b_size == 0 {
            // doesn't preserve address invariant if a_size != 0
            let mut v = Vec::with_capacity(0);

            for e in self.into_iter() {
                v.push(f(e));
            }

            v
        } else {
            let cap = if a_size == b_size {
                self.capacity()
            } else if a_size > b_size {
                // nA * bytes/A = nbytes
                // nbytes / bytes/B = nbytes * B/bytes = nB
                // (assuming bytes/B divides evenly into nbytes)
                let n_bytes = self.capacity().checked_mul(a_size).unwrap();
                // TODO: don't require the divisibility constraint
                assert_eq!(n_bytes % b_size, 0);
                n_bytes / b_size
            } else {
                panic!("map_in_place(Vec<A>): Size of A must be greater than or equal to size of B")
            };

            let mut dropper = VecDropper {
                idx: 0,
                owned: self,
                _marker: PhantomData::<B>,
            };

            unsafe {
                for i in 0..len {
                    let ptr_a = ptr_a.offset(i as isize);
                    let ptr_b = ptr_b.offset(i as isize);

                    let v = ptr::read(ptr_a);
                    dropper.idx += 1;

                    ptr::write(ptr_b, f(v));
                }

                Vec::from_raw_parts(ptr_b, len, cap)
            }
        }
    }
}

struct VecDropper<A, B> {
    idx: usize,
    owned: Vec<A>,
    _marker: PhantomData<B>,
}

impl<A, B> Drop for VecDropper<A, B> {
    #[inline]
    fn drop(&mut self) {
        let owned = &mut self.owned;
        let idx = self.idx;
        let len = owned.len();
        let ptr_a = owned.as_mut_ptr();
        let ptr_b = ptr_a as *mut B;

        unsafe {
            owned.set_len(0);

            if idx != len {
                // panicked; manual cleanup needed
                for i in 0..(idx - 1) {
                    ptr::drop_in_place(ptr_b.offset(i as isize));
                }

                for i in idx..len {
                    ptr::drop_in_place(ptr_a.offset(i as isize));
                }
            } else {
                // everything went well, no cleanup required
                mem::forget(mem::replace(owned, Vec::with_capacity(0)));
            }
        }
    }
}

// TODO: more tests (panicking with different size cfgs), more impls

#[cfg(test)]
mod tests {
    use super::MapInPlace;

    use std::mem;
    use std::sync::Mutex;
    use std::panic::catch_unwind;

    #[test]
    fn vec_elements_drop() {
        lazy_static! {
            static ref DROPS: Mutex<Vec<String>> = Mutex::new(vec![]);
        }

        #[derive(Debug, PartialEq, Clone)]
        struct X(usize);

        impl Drop for X {
            fn drop(&mut self) {
                DROPS.lock().unwrap().push(format!("X({})", self.0));
            }
        }

        #[derive(Debug, PartialEq, Clone)]
        struct Y(usize);

        impl Drop for Y {
            fn drop(&mut self) {
                DROPS.lock().unwrap().push(format!("Y({})", self.0));
            }
        }

        assert_eq!(mem::size_of::<X>(), mem::size_of::<Y>());

        let v = vec![X(0), X(1), X(2), X(3)];

        let bp = v.as_ptr() as *const ();
        let v = v.map_in_place(|X(v)| Y(v));

        {
            let drops = DROPS.lock().unwrap().clone();
            assert_eq!(drops, vec!["X(0)", "X(1)", "X(2)", "X(3)"]);
        }

        let ap = v.as_ptr() as *const ();
        let expected = vec![Y(0), Y(1), Y(2), Y(3)];

        assert_eq!(bp, ap); // still at same memory addr
        assert_eq!(v, expected);

        mem::drop(v);

        {
            let drops = DROPS.lock().unwrap().clone();
            assert_eq!(drops,
                       vec!["X(0)", "X(1)", "X(2)", "X(3)", "Y(0)", "Y(1)", "Y(2)", "Y(3)"]);
        }

        mem::drop(expected);
    }

    macro_rules! vec_panic_drop_test {
        ($name:ident, $ytype:ty) => {
            #[test]
            fn $name() {
                lazy_static! {
                    static ref DROPS: Mutex<Vec<String>> = Mutex::new(vec![]);
                }
                
                #[derive(Debug, PartialEq, Clone)]
                struct X(char);
                
                impl Drop for X {
                    fn drop(&mut self) {
                        DROPS.lock().unwrap().push(format!("X({})", self.0));
                    }
                }
                
                #[derive(Debug, PartialEq, Clone)]
                struct Y($ytype);
                
                impl Drop for Y {
                    fn drop(&mut self) {
                        DROPS.lock().unwrap().push(format!("Y({})", self.0));
                    }
                }
                
                let v = vec![X('a'), X('b'), X('c'), X('d'), X('e')];
                
                match catch_unwind(|| {
                    v.map_in_place(|X(v)| {
                        if v == 'c' {
                            panic!();
                        } else {
                            Y(v as $ytype)
                        }
                    })
                }) {
                    Ok(_) => unreachable!(),
                    Err(_) => {
                        let drops = DROPS.lock().unwrap().clone();
                        assert_eq!(drops,
                                   vec![// consume Xs
                                       "X(a)",
                                       "X(b)",
                                       "X(c)",
                                       // panic here
                                       // drop generated Ys
                                       "Y(97)",
                                       "Y(98)",
                                       // drop remaining unprocessed Xs
                                       "X(d)",
                                       "X(e)"]);
                    }
                }
            }
        }
    }

    vec_panic_drop_test!(vec_same_size_panic_drop, u32);
    vec_panic_drop_test!(vec_diff_size_panic_drop, u16);

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
}
