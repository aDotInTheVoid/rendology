mod mesh;
mod scene;

use std::ops::{Index, IndexMut};

use glium::implement_vertex;
use num_traits::{FromPrimitive, ToPrimitive};

use crate::shader::{InstanceInput, InstancingMode, ToUniforms};
use crate::{CreationError, DrawError, Drawable, Mesh};

pub use mesh::{load_wavefront, mesh_from_slices, CUBE_INDICES, CUBE_NORMALS, CUBE_POSITIONS};
pub use scene::{Core, Instance};

#[derive(Copy, Clone, PartialEq, Eq, Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum BasicObj {
    Triangle,
    Quad,
    Cube,
    Sphere,

    LineX,
    LineY,
    LineZ,

    TessellatedCube,
    TessellatedCylinder,
}

pub const NUM_TYPES: usize = 9;

#[derive(Copy, Clone, Debug)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
}

implement_vertex!(Vertex, position, normal);

pub struct Resources {
    pub meshes: Vec<Mesh<Vertex>>,
}

impl Resources {
    pub fn create<F: glium::backend::Facade>(facade: &F) -> Result<Resources, CreationError> {
        // Unfortunately, it doesn't seem easy to use enum_map here,
        // since we need to check for errors in creating buffers
        let mut meshes = Vec::new();

        for i in 0..NUM_TYPES {
            // Safe to unwrap here, since we iterate within the range
            let object: BasicObj = FromPrimitive::from_usize(i).unwrap();

            meshes.push(object.create_mesh(facade)?);
        }

        Ok(Resources { meshes })
    }

    pub fn mesh(&self, object: BasicObj) -> &Mesh<Vertex> {
        // Safe to unwrap since `BasicObj::to_usize()` never fails.
        &self.meshes[object.to_usize().unwrap()]
    }
}

impl BasicObj {
    #[rustfmt::skip]
    pub fn create_mesh<F: glium::backend::Facade>(
        self,
        facade: &F,
    ) -> Result<Mesh<Vertex>, CreationError> {
        mesh::create_mesh(self, facade)
    }
}

pub struct RenderList<I: InstanceInput>(Vec<crate::RenderList<I>>);

impl<I: InstanceInput> RenderList<I> {
    pub fn clear(&mut self) {
        for list in self.0.iter_mut() {
            list.clear();
        }
    }

    pub fn as_drawable<'a>(&'a self, resources: &'a Resources) -> impl Drawable<I, Vertex> + 'a {
        RenderListDrawableImpl(self, resources)
    }
}

impl<I: InstanceInput + Clone> Default for RenderList<I> {
    fn default() -> Self {
        Self(vec![Default::default(); NUM_TYPES])
    }
}

impl<I: InstanceInput> Index<BasicObj> for RenderList<I> {
    type Output = crate::RenderList<I>;

    fn index(&self, object: BasicObj) -> &crate::RenderList<I> {
        // Safe to unwrap since `BasicObj::to_usize()` never fails.
        &self.0[object.to_usize().unwrap()]
    }
}

impl<I: InstanceInput> IndexMut<BasicObj> for RenderList<I> {
    fn index_mut(&mut self, object: BasicObj) -> &mut crate::RenderList<I> {
        // Safe to unwrap since `BasicObj::to_usize()` never fails.
        &mut self.0[object.to_usize().unwrap()]
    }
}

pub struct Instancing<I: InstanceInput>(Vec<crate::Instancing<I>>);

impl<I: InstanceInput> Instancing<I> {
    pub fn create<F: glium::backend::Facade>(facade: &F) -> Result<Self, CreationError> {
        let mut vec = Vec::new();
        for _ in 0..NUM_TYPES {
            vec.push(crate::Instancing::create(facade)?);
        }

        Ok(Instancing(vec))
    }

    pub fn update<F: glium::backend::Facade>(
        &mut self,
        facade: &F,
        render_list: &RenderList<I>,
    ) -> Result<(), CreationError> {
        for i in 0..NUM_TYPES {
            self.0[i].update(facade, render_list.0[i].as_slice())?;
        }

        Ok(())
    }

    pub fn as_drawable<'a>(&'a self, resources: &'a Resources) -> impl Drawable<I, Vertex> + 'a {
        InstancingDrawableImpl(self, resources)
    }
}

struct InstancingDrawableImpl<'a, I: InstanceInput>(&'a Instancing<I>, &'a Resources);

impl<'a, I: InstanceInput> Drawable<I, Vertex> for InstancingDrawableImpl<'a, I> {
    const INSTANCING_MODE: InstancingMode = InstancingMode::Vertex;

    fn draw<U, S>(
        &self,
        program: &glium::Program,
        uniforms: &U,
        draw_params: &glium::DrawParameters,
        target: &mut S,
    ) -> Result<(), DrawError>
    where
        U: ToUniforms,
        S: glium::Surface,
    {
        for i in 0..NUM_TYPES {
            (self.0).0[i].as_drawable(&self.1.meshes[i]).draw(
                program,
                uniforms,
                draw_params,
                target,
            )?;
        }

        Ok(())
    }
}

struct RenderListDrawableImpl<'a, I: InstanceInput>(&'a RenderList<I>, &'a Resources);

impl<'a, I: InstanceInput> Drawable<I, Vertex> for RenderListDrawableImpl<'a, I> {
    const INSTANCING_MODE: InstancingMode = InstancingMode::Uniforms;

    fn draw<U, S>(
        &self,
        program: &glium::Program,
        uniforms: &U,
        draw_params: &glium::DrawParameters,
        target: &mut S,
    ) -> Result<(), DrawError>
    where
        U: ToUniforms,
        S: glium::Surface,
    {
        for i in 0..NUM_TYPES {
            (self.0).0[i].as_drawable(&self.1.meshes[i]).draw(
                program,
                uniforms,
                draw_params,
                target,
            )?;
        }

        Ok(())
    }
}
