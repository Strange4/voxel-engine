# Things I would do if I had infinite time

1. Remove the vulkano_shaders crate and compile the shaders into SPIR-V using naga instead to not have to build the _entire_ shaderc library (quite ass since they don't provide binaries anymore).

2. Make a PR into vulkano so that the reflection step of SPIR-V (which was just compiled and verified btw) doesn't take 1MB of stack space. This overflows the default stack size on windows. It shouldn't take that much space.
