use std::fs;
use std::io;
use std::path::Path as Path;
use std::collections::HashMap;
use std::cell::{RefCell, RefMut};
use std::any::{TypeId, Any};
use std::borrow::{Borrow};
use std::marker::PhantomData;
use std::sync::Arc;
use std::ops::{SubAssign, AddAssign};

static mut BASE_ASSET_PATH: &str = "./assets/";

pub trait LoadableAsset
where Self : Sized {
    fn read(path: &str) -> io::Result<Self>;
}

pub fn get_dir(path: &str) -> String {
    match path.find('/') {
        Some(ix) => String::from(&path[0..ix]),
        None => String::new()
    }
}

#[inline]
pub fn load_asset<T>(path: &str) -> io::Result<T>
where T: LoadableAsset
{
    info!("load_asset: {:?}", &path);
    return T::read(path);
}

#[inline]
pub fn load_asset_local<T>(base_dir: &str, path: &str) -> io::Result<T>
where T: LoadableAsset
{
    let p = get_asset_path_local(base_dir, path);
    info!("load_asset: {:?}", &p);
    return T::read(p.as_str());
}

#[inline]
pub fn get_asset_path_local(base_dir: &str, path: &str) -> String {
    if base_dir.is_empty() {
        String::from(path)
    } else {
        format!("{ }/{}", base_dir, path)
    }
}

impl LoadableAsset for String {
    fn read(path: &str) -> io::Result<Self> {
        fs::read_to_string(get_fs_path(path))
    }
}

impl LoadableAsset for Vec<u8> {
    fn read(path: &str) -> io::Result<Self> {
        fs::read(get_fs_path(path))
    }
}

pub fn set_base_asset_path(path: &'static str) {
    unsafe {
        BASE_ASSET_PATH = path;
    }
}

fn get_fs_path(path: &str) -> Box<Path> {
    return Path::new(unsafe { BASE_ASSET_PATH }).join(path).into_boxed_path();
}

pub struct ResourceRef<T> {
    idx: usize,
    type_id: TypeId,
    ref_cnt: Arc<u32>,
    marker: PhantomData<T>
}

// PhantomData 只当做一个类型标记，实际上能够跨线程同步
unsafe impl<T> Send for ResourceRef<T> {}
unsafe impl<T> Sync for ResourceRef<T> {}

impl<T> Drop for ResourceRef<T> {
    fn drop(&mut self) {
        *Arc::get_mut(&mut self.ref_cnt).unwrap() -= 1;
    }
}

impl<T> Clone for ResourceRef<T> {
    fn clone(&self) -> Self {
        let mut ret = Self {
            idx: self.idx,
            type_id: self.type_id,
            marker: PhantomData,
            ref_cnt: self.ref_cnt.clone()
        };

        *Arc::get_mut(&mut ret.ref_cnt).unwrap() += 1;

        ret
    }
}

struct ResourceEntry<T> {
    resource: T,
    ref_cnt: Arc<u32>
}

pub struct ResourcePool<T> where T: 'static {
    entries: Vec<Option<ResourceEntry<T>>>,
    free_indices: Vec<usize>,
}

impl<T> ResourcePool<T> where T: 'static {

    pub fn new() -> Self {
        Self {
            entries: vec![],
            free_indices: vec![]
        }
    }

    pub fn add(&mut self, res: T) -> ResourceRef<T> {
        let ref_cnt = Arc::new(1);
        let resource_entry = ResourceEntry {
            resource: res,
            ref_cnt: ref_cnt.clone()
        };
        let idx = if self.free_indices.is_empty() {
            self.entries.push(Some(resource_entry));
            self.entries.len() - 1
        } else {
            let idx = self.free_indices.remove(self.free_indices.len() - 1);
            self.entries[idx] = Some(resource_entry);
            idx
        };

        ResourceRef {
            idx,
            type_id: TypeId::of::<T>(),
            ref_cnt,
            marker: PhantomData
        }
    }

    pub fn get(&self, res_ref: &ResourceRef<T>) -> &T {
        & (&self.entries[res_ref.idx]).as_ref().unwrap().resource
    }

    pub fn get_mut(&mut self, res_ref: &ResourceRef<T>) -> &mut T {
        &mut (&mut self.entries[res_ref.idx]).as_mut().unwrap().resource
    }
}

pub fn add_local_resource<T>(res: T) -> ResourceRef<T>
where T: 'static
{
    let type_id = TypeId::of::<T>();
    ALL_RESOURCES.with(|ref_cell| {
        let mut map = ref_cell.borrow_mut();
        if !map.contains_key(&type_id) {
            let hash_map: HashMap<TypeId, ResourcePool<T>> = HashMap::new();
            map.insert(type_id, Box::new(hash_map));
        }

        let pool: &mut ResourcePool<T> = map.get_mut(&type_id).unwrap().downcast_mut().unwrap();
        pool.add(res)
    })
}

pub fn with_local_resource<T, F>(res_ref: &ResourceRef<T>, f: F)
where F: FnOnce(&mut T), T: 'static
{
    ALL_RESOURCES.with(|ref_cell| {
        let mut map = ref_cell.borrow_mut();
        let pool: &mut ResourcePool<T> = map.get_mut(&res_ref.type_id).unwrap().downcast_mut().unwrap();
        let res = &mut pool.entries[res_ref.idx].as_mut().unwrap().resource;
        f(res);
    });
}

pub fn with_local_resource_pool<T, F>(f: F)
where F: FnOnce(&mut ResourcePool<T>), T: 'static
{
    let type_id = TypeId::of::<T>();
    ALL_RESOURCES.with(|ref_cell| {
        let mut map = ref_cell.borrow_mut();
        let pool: &mut ResourcePool<T> = map.get_mut(&type_id).unwrap().downcast_mut().unwrap();
        f(pool);
    });
}

thread_local! {
static ALL_RESOURCES: RefCell<HashMap<TypeId, Box<dyn Any>>> = RefCell::new(HashMap::new());
}