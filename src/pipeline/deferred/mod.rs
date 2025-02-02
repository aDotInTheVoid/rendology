//! Deferred shading.
//!
//! Heavily inspired by:
//! https://github.com/glium/glium/blob/master/examples/deferred.rs

pub mod shaders;

use log::info;

use nalgebra as na;

use glium::{uniform, Surface, Texture2d};

use crate::shader::{self, InstanceInput, ToUniforms};
use crate::{
    basic_obj, screen_quad, BasicObj, Camera, Context, DrawError, Drawable, Instancing, Light,
    Mesh, ScreenQuad,
};

use crate::pipeline::render_pass::{
    CompositionPassComponent, HasCompositionPassParams, HasScenePassParams, RenderPassComponent,
    ScenePassComponent,
};

pub use crate::CreationError;

#[derive(Debug, Clone)]
pub struct Config {
    pub light_min_threshold: f32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            light_min_threshold: 0.02,
        }
    }
}

const NUM_TEXTURES: usize = 2;

pub struct DeferredShading {
    config: Config,

    scene_textures: [Texture2d; NUM_TEXTURES],
    shadow_texture: Option<Texture2d>,

    light_texture: Texture2d,

    main_light_screen_quad_program: glium::Program,
    light_object_program: glium::Program,

    screen_quad: ScreenQuad,
    sphere: Mesh<basic_obj::Vertex>,

    light_instances: Vec<<Light as InstanceInput>::Vertex>,
    light_instancing: Instancing<Light>,
}

impl RenderPassComponent for DeferredShading {
    fn clear_buffers<F: glium::backend::Facade>(&self, facade: &F) -> Result<(), DrawError> {
        let mut framebuffer = glium::framebuffer::MultiOutputFrameBuffer::new(
            facade,
            self.output_textures().iter().cloned(),
        )?;
        framebuffer.clear_color(0.0, 0.0, 0.0, 1.0);

        Ok(())
    }
}

impl<'u> HasScenePassParams<'u> for DeferredShading {
    type Params = ();
}

impl ScenePassComponent for DeferredShading {
    fn core_transform<P, I, V>(
        &self,
        core: shader::Core<(Context, P), I, V>,
    ) -> shader::Core<(Context, P), I, V> {
        // Write scene to separate buffers
        shaders::scene_buffers_core_transform(self.shadow_texture.is_some(), core)
    }

    fn output_textures(&self) -> Vec<(&'static str, &Texture2d)> {
        let mut result = vec![
            ("f_world_pos", &self.scene_textures[0]),
            ("f_world_normal", &self.scene_textures[1]),
        ];

        if let Some(shadow_texture) = self.shadow_texture.as_ref() {
            result.push(("f_shadow", shadow_texture));
        }

        result
    }

    fn params(&self, _: &Context) {}
}

pub struct CompositionPassParams<'a> {
    light_texture: &'a Texture2d,
    normal_texture: &'a Texture2d,
}

impl_uniform_input!(
    CompositionPassParams<'a>,
    self => {
        light_texture: &'a Texture2d = self.light_texture,
        normal_texture: &'a Texture2d = self.normal_texture,
    },
);

impl<'u> HasCompositionPassParams<'u> for DeferredShading {
    type Params = CompositionPassParams<'u>;
}

impl CompositionPassComponent for DeferredShading {
    fn core_transform(
        &self,
        core: shader::Core<Context, (), screen_quad::Vertex>,
    ) -> shader::Core<Context, (), screen_quad::Vertex> {
        shaders::composition_core_transform(core)
    }

    fn params(&self) -> CompositionPassParams {
        CompositionPassParams {
            light_texture: &self.light_texture,
            normal_texture: &self.scene_textures[1],
        }
    }
}

impl DeferredShading {
    pub fn create<F: glium::backend::Facade>(
        facade: &F,
        config: &Config,
        have_shadows: bool,
        target_size: (u32, u32),
    ) -> Result<DeferredShading, CreationError> {
        info!("Creating deferred buffer textures");
        let scene_textures = [
            Self::create_texture(facade, target_size)?,
            Self::create_texture(facade, target_size)?,
        ];
        let shadow_texture = if have_shadows {
            Some(Self::create_shadow_texture(facade, target_size)?)
        } else {
            None
        };
        let light_texture = Self::create_texture(facade, target_size)?;

        info!("Creating deferred light programs");
        let main_light_screen_quad_core = shaders::main_light_screen_quad_core(have_shadows);
        let main_light_screen_quad_program =
            main_light_screen_quad_core.build_program(facade, shader::InstancingMode::Uniforms)?;
        let light_object_core = shaders::light_object_core();
        let light_object_program =
            light_object_core.build_program(facade, shader::InstancingMode::Vertex)?;

        info!("Creating screen quad");
        let screen_quad = ScreenQuad::create(facade)?;

        info!("Creating sphere");
        let sphere = BasicObj::Sphere.create_mesh(facade)?;

        info!("Creating light buffers");
        let light_instancing = Instancing::create(facade)?;

        info!("Deferred shading initialized");

        Ok(DeferredShading {
            config: config.clone(),
            scene_textures,
            shadow_texture,
            light_texture,
            main_light_screen_quad_program,
            light_object_program,
            screen_quad,
            sphere,
            light_instances: Vec::new(),
            light_instancing,
        })
    }

    pub fn on_target_resize<F: glium::backend::Facade>(
        &mut self,
        facade: &F,
        target_size: (u32, u32),
    ) -> Result<(), CreationError> {
        info!(
            "Recreating textures for deferred shading with size {:?}",
            target_size,
        );

        self.scene_textures = [
            Self::create_texture(facade, target_size)?,
            Self::create_texture(facade, target_size)?,
        ];

        if let Some(shadow_texture) = self.shadow_texture.as_mut() {
            *shadow_texture = Self::create_shadow_texture(facade, target_size)?;
        }

        self.light_texture = Self::create_texture(facade, target_size)?;

        Ok(())
    }

    pub fn light_pass<F: glium::backend::Facade>(
        &mut self,
        facade: &F,
        camera: &Camera,
        lights: &[Light],
    ) -> Result<(), DrawError> {
        let draw_params = glium::DrawParameters {
            backface_culling: glium::draw_parameters::BackfaceCullingMode::CullClockwise,
            blend: glium::Blend {
                color: glium::BlendingFunction::Addition {
                    source: glium::LinearBlendingFactor::One,
                    destination: glium::LinearBlendingFactor::One,
                },
                alpha: glium::BlendingFunction::Addition {
                    source: glium::LinearBlendingFactor::One,
                    destination: glium::LinearBlendingFactor::One,
                },
                constant_value: (1.0, 1.0, 1.0, 1.0),
            },
            ..Default::default()
        };

        let mut light_buffer =
            glium::framebuffer::SimpleFrameBuffer::new(facade, &self.light_texture)?;

        light_buffer.clear_color(0.0, 0.0, 0.0, 1.0);

        let textures = (
            &uniform! {
                position_texture: &self.scene_textures[0],
                normal_texture: &self.scene_textures[1],
            },
            &self.shadow_texture.as_ref().map(|shadow_texture| {
                plain_uniforms! {
                    shadow_texture: shadow_texture,
                }
            }),
        );

        self.light_instances.clear();
        for light in lights {
            if light.is_main {
                continue;
            }

            let i_max = light.color.x.max(light.color.y).max(light.color.z);
            let radicand = light.attenuation.y.powi(2)
                - 4.0
                    * light.attenuation.z
                    * (light.attenuation.x - i_max * 1.0 / self.config.light_min_threshold);
            let radius = (-light.attenuation.y + radicand.sqrt()) / (2.0 * light.attenuation.z);

            let light = Light {
                radius,
                ..light.clone()
            };

            self.light_instances.push(light.to_vertex());
        }

        self.light_instancing
            .update(facade, &self.light_instances)?;

        // Draw main light
        for light in lights.iter() {
            if light.is_main {
                // Fragment shader uses viewport size, but we don't need view/projection
                let no_camera = Camera {
                    view: na::Matrix4::identity(),
                    projection: na::Matrix4::identity(),
                    viewport_size: camera.viewport_size,
                };

                let uniforms = (&textures, (no_camera, &light));

                light_buffer.draw(
                    &self.screen_quad.vertex_buffer,
                    &self.screen_quad.index_buffer,
                    &self.main_light_screen_quad_program,
                    &uniforms.to_uniforms(),
                    &draw_params,
                )?;
            }
        }

        // Draw additional light using instancing
        let uniforms = (&textures, &camera);

        // With backface culling, there is a problem in that lights are
        // not rendered when the camera moves within the sphere. With
        // frontface culling this problem does not happen.
        // (I think there's some other downside, but I'm not sure what
        // it is exactly.)
        let draw_params = glium::DrawParameters {
            backface_culling: glium::draw_parameters::BackfaceCullingMode::CullCounterClockwise,
            ..draw_params.clone()
        };

        self.light_instancing.as_drawable(&self.sphere).draw(
            &self.light_object_program,
            &uniforms,
            &draw_params,
            &mut light_buffer,
        )?;

        Ok(())
    }

    fn create_texture<F: glium::backend::Facade>(
        facade: &F,
        size: (u32, u32),
    ) -> Result<Texture2d, CreationError> {
        Ok(Texture2d::empty_with_format(
            facade,
            glium::texture::UncompressedFloatFormat::F32F32F32F32,
            glium::texture::MipmapsOption::NoMipmap,
            size.0,
            size.1,
        )?)
    }

    fn create_shadow_texture<F: glium::backend::Facade>(
        facade: &F,
        size: (u32, u32),
    ) -> Result<Texture2d, CreationError> {
        Ok(Texture2d::empty_with_format(
            facade,
            glium::texture::UncompressedFloatFormat::F32,
            glium::texture::MipmapsOption::NoMipmap,
            size.0,
            size.1,
        )?)
    }
}
