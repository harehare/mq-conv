use lopdf::{Dictionary, Document, Object};

/// Follow indirect references to the underlying object.
pub fn resolve<'a>(doc: &'a Document, obj: &'a Object) -> &'a Object {
    let mut cur = obj;
    for _ in 0..16 {
        match cur {
            Object::Reference(id) => match doc.get_object(*id) {
                Ok(o) => cur = o,
                Err(_) => return &Object::Null,
            },
            _ => return cur,
        }
    }
    cur
}

pub fn dict_get<'a>(doc: &'a Document, dict: &'a Dictionary, key: &[u8]) -> Option<&'a Object> {
    dict.get(key).ok().map(|o| resolve(doc, o))
}

pub fn num(obj: &Object) -> Option<f32> {
    match obj {
        Object::Integer(i) => Some(*i as f32),
        Object::Real(r) => Some(*r),
        _ => None,
    }
}
