//! Compiling and linking the GL programs.
use super::*;

pub(super) fn gl_string(name: GLenum) -> String {
    unsafe {
        let value = gl::GetString(name);
        if value.is_null() {
            "unknown".into()
        } else {
            CStr::from_ptr(value.cast()).to_string_lossy().into_owned()
        }
    }
}

fn compile(kind: GLenum, source: &str) -> Result<GLuint, String> {
    let source = CString::new(source).map_err(|e| e.to_string())?;
    unsafe {
        let shader = gl::CreateShader(kind);
        gl::ShaderSource(shader, 1, &source.as_ptr(), std::ptr::null());
        gl::CompileShader(shader);
        let mut success = 0;
        gl::GetShaderiv(shader, gl::COMPILE_STATUS, &mut success);
        if success == 0 {
            let mut log = vec![0u8; 4096];
            let mut len = 0;
            gl::GetShaderInfoLog(shader, log.len() as i32, &mut len, log.as_mut_ptr().cast());
            gl::DeleteShader(shader);
            return Err(format!(
                "Shader compilation: {}",
                String::from_utf8_lossy(&log[..len as usize])
            ));
        }
        Ok(shader)
    }
}

pub(super) fn link(vertex: &str, fragment: &str, attributes: &[&CStr]) -> Result<GLuint, String> {
    let vertex = compile(gl::VERTEX_SHADER, vertex)?;
    let fragment = match compile(gl::FRAGMENT_SHADER, fragment) {
        Ok(fragment) => fragment,
        Err(error) => {
            unsafe {
                gl::DeleteShader(vertex);
            }
            return Err(error);
        }
    };
    unsafe {
        let program = gl::CreateProgram();
        gl::AttachShader(program, vertex);
        gl::AttachShader(program, fragment);
        for (index, name) in attributes.iter().enumerate() {
            gl::BindAttribLocation(program, index as u32, name.as_ptr());
        }
        gl::LinkProgram(program);
        gl::DeleteShader(vertex);
        gl::DeleteShader(fragment);
        let mut success = 0;
        gl::GetProgramiv(program, gl::LINK_STATUS, &mut success);
        if success == 0 {
            let mut log = vec![0u8; 4096];
            let mut len = 0;
            gl::GetProgramInfoLog(program, log.len() as i32, &mut len, log.as_mut_ptr().cast());
            gl::DeleteProgram(program);
            return Err(format!(
                "Shader linking: {}",
                String::from_utf8_lossy(&log[..len as usize])
            ));
        }
        Ok(program)
    }
}
