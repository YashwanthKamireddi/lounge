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

/// Linked programs kept on the TV's flash (GL_OES_get_program_binary), so a cold start loads
/// them instead of compiling: compiling and linking the six programs is most of the ~300 ms
/// "shaders and setup" stage of every start as Home. A program's file is named for its sources
/// and the driver, so a new version or a firmware update compiles afresh; a binary the driver
/// refuses is compiled too.
pub(super) struct ProgramCache {
    directory: Option<std::path::PathBuf>,
    driver: String,
    get: Option<GetProgramBinary>,
    load: Option<ProgramBinary>,
    used: Vec<std::ffi::OsString>,
    writes: Vec<(std::path::PathBuf, Vec<u8>)>,
    pub loaded: u32,
    pub compiled: u32,
}

type GetProgramBinary =
    unsafe extern "C" fn(GLuint, GLsizei, *mut GLsizei, *mut GLenum, *mut std::ffi::c_void);
type ProgramBinary = unsafe extern "C" fn(GLuint, GLenum, *const std::ffi::c_void, GLint);

const PROGRAM_BINARY_LENGTH_OES: GLenum = 0x8741;
const NUM_PROGRAM_BINARY_FORMATS_OES: GLenum = 0x87FE;

/// FNV-1a: stable across builds, unlike std's hasher.
fn fingerprint(parts: &[&[u8]]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for part in parts {
        for byte in part.iter().chain(&[0xff]) {
            hash = (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    hash
}

impl ProgramCache {
    /// `directory` None keeps nothing (the desktop, where compiling is quick).
    pub fn new(directory: Option<&std::path::Path>) -> Self {
        let mut cache = Self {
            directory: None,
            driver: String::new(),
            get: None,
            load: None,
            used: Vec::new(),
            writes: Vec::new(),
            loaded: 0,
            compiled: 0,
        };
        let Some(directory) = directory else {
            return cache;
        };
        let extensions = gl_string(gl::EXTENSIONS);
        let mut formats = 0;
        unsafe { gl::GetIntegerv(NUM_PROGRAM_BINARY_FORMATS_OES, &mut formats) };
        if !extensions
            .split_ascii_whitespace()
            .any(|name| name == "GL_OES_get_program_binary")
            || formats < 1
        {
            return cache;
        }
        let address = |name: &CStr| unsafe { sdl2::sys::SDL_GL_GetProcAddress(name.as_ptr()) };
        let (get, load) = (
            address(c"glGetProgramBinaryOES"),
            address(c"glProgramBinaryOES"),
        );
        if get.is_null() || load.is_null() {
            return cache;
        }
        // SAFETY: the extension is present, and these are its entry points with its signatures.
        unsafe {
            cache.get = Some(std::mem::transmute::<*mut std::ffi::c_void, GetProgramBinary>(get));
            cache.load = Some(std::mem::transmute::<*mut std::ffi::c_void, ProgramBinary>(
                load,
            ));
        }
        cache.driver = [gl::VENDOR, gl::RENDERER, gl::VERSION]
            .map(gl_string)
            .join("\n");
        cache.directory = Some(directory.to_path_buf());
        cache
    }

    pub fn link(
        &mut self,
        vertex: &str,
        fragment: &str,
        attributes: &[&CStr],
    ) -> Result<GLuint, String> {
        let (Some(directory), Some(get), Some(load)) = (&self.directory, self.get, self.load)
        else {
            self.compiled += 1;
            return link(vertex, fragment, attributes);
        };
        let names: Vec<&[u8]> = attributes.iter().map(|name| name.to_bytes()).collect();
        let mut parts = vec![
            self.driver.as_bytes(),
            vertex.as_bytes(),
            fragment.as_bytes(),
        ];
        parts.extend(names);
        let name = std::ffi::OsString::from(format!("{:016x}.bin", fingerprint(&parts)));
        let path = directory.join(&name);
        self.used.push(name);
        if let Ok(bytes) = std::fs::read(&path)
            && bytes.len() > 4
        {
            let format = GLenum::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            let binary = &bytes[4..];
            unsafe {
                let program = gl::CreateProgram();
                load(
                    program,
                    format,
                    binary.as_ptr().cast(),
                    binary.len() as GLint,
                );
                let mut success = 0;
                gl::GetProgramiv(program, gl::LINK_STATUS, &mut success);
                if success != 0 {
                    self.loaded += 1;
                    return Ok(program);
                }
                gl::DeleteProgram(program);
            }
        }
        let program = link(vertex, fragment, attributes)?;
        self.compiled += 1;
        unsafe {
            let mut length = 0;
            gl::GetProgramiv(program, PROGRAM_BINARY_LENGTH_OES, &mut length);
            if length > 0 {
                let mut bytes = vec![0u8; 4 + length as usize];
                let (mut written, mut format) = (0, 0);
                get(
                    program,
                    length,
                    &mut written,
                    &mut format,
                    bytes[4..].as_mut_ptr().cast(),
                );
                if written > 0 {
                    bytes.truncate(4 + written as usize);
                    bytes[..4].copy_from_slice(&format.to_le_bytes());
                    self.writes.push((path, bytes));
                }
            }
        }
        Ok(program)
    }

    /// Writes the new binaries and removes those no program uses any more (an older version's),
    /// on a worker: flash writes stay off the way to the first frame.
    pub fn store(self) {
        let Some(directory) = self.directory else {
            return;
        };
        if self.writes.is_empty() && self.loaded as usize == self.used.len() {
            return;
        }
        std::thread::spawn(move || {
            if std::fs::create_dir(&directory).is_ok() {
                let _ = crate::storage::give_to_folder_owner(&directory);
            }
            for (path, bytes) in self.writes {
                let temporary = path.with_extension("tmp");
                let written = std::fs::write(&temporary, &bytes)
                    .and_then(|()| crate::storage::give_to_folder_owner(&temporary))
                    .and_then(|()| std::fs::rename(&temporary, &path));
                if let Err(error) = written {
                    eprintln!("Lounge shader cache {}: {error}", path.display());
                    let _ = std::fs::remove_file(&temporary);
                }
            }
            for entry in std::fs::read_dir(&directory)
                .into_iter()
                .flatten()
                .flatten()
            {
                if !self.used.contains(&entry.file_name()) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        });
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

#[cfg(test)]
mod tests {
    use super::fingerprint;

    #[test]
    fn a_program_file_name_follows_every_source_and_their_boundaries() {
        let base = fingerprint(&[b"driver", b"vertex", b"fragment"]);
        assert_eq!(base, fingerprint(&[b"driver", b"vertex", b"fragment"]));
        assert_ne!(base, fingerprint(&[b"driver 2", b"vertex", b"fragment"]));
        assert_ne!(base, fingerprint(&[b"driver", b"vertexf", b"ragment"]));
        // Stable across builds: the file written by one version is found by the next.
        assert_eq!(fingerprint(&[]), 0xcbf2_9ce4_8422_2325);
    }
}
