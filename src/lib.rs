mod parser;

use pyo3::prelude::*;
// use parser::Parser;
use std::collections::HashMap;

#[pyclass]
struct Parser {
    num: i32,
}

#[pyclass]
pub struct Node {
    pub id: u64,
    pub tags: HashMap<String, String>,
    pub lat: f64,
    pub lon: f64,
}

#[pyclass]
pub struct PublicTransport {
    pub id: u64,
    pub tags: HashMap<String, String>,
    pub stops: Vec<Node>,
    pub geometry: Vec<Vec<(f64, f64)>>, // lon, lat
}

#[pymethods]
impl Parser {

    #[new]
    fn new(obj: &PyRawObject, num: i32) {
        obj.init({
            Parser {
                num,
            }
        });
    }
}

/// This module is a python module implemented in Rust.
#[pymodule]
fn osmptparser(py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<Parser>()?;
    m.add_class::<PublicTransport>()?;

    Ok(())
}
