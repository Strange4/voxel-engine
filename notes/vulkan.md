#### Why do we have to align and pad the layout for the push constants?

The way that rust represents a struct is packed as closely as possible, eg: `struct A { a: Vec3, b: Vec3 }` will take `(32/8) * 3 * 2 = 24 bytes` since all the 6 floats are next to each other. However, the data representation of the std140 layout will padd according to [some rules](https://learnopengl.com/Advanced-OpenGL/Advanced-GLSL). These rules are that the starting offset for an element must be a multiple of its padded size. For example, a vec3's padded size is the size of a vec4, so the offset of the second vec3 will be 16 bytes. However, they will **NOT** pad at the end. So a struct of 2 vec3's will have a total size of 16 + 12 = 28 bytes.

#### What is a stencil image?

When you are making a grafiti, you use a stencil to "mask" the shape that you want to draw on. Imagine it like letters that are being painted on a cargo container.
A stencil image in vulkan is an image that is used like a mask to draw other images. Like a stencil in real life.

#### What are descriptor sets?

A descriptor is an object that "describes" some resource used by a shader pipeline. For example, a descriptor for an image could describe the color type, the size and other factors.
When you pass those resources to the pipeline they aren't passed 1 by 1, they are grouped into descriptor sets and then the whole set is passed to the pipeline to be used. Then each element of that set has a "binding" that can be bound to a variable in the shader.

###### What are descriptor sets layouts?

Read up on previous question. The layout of a descriptor set is an array of all the bindings of the descriptor sets and they type.

#### Why a descriptor set per swapchain image?

The idea of a swapchain is that you start rendering the next frame while the current frame is still being "presented" and viewed on the screen. There can be many images being rendered while only 1 of them is being presented. You typically want to modify the data that those images are being rendered between frames (like animation, changing the verteces, color palletes, etc.) so that the next frame shows something different. But if you're modifying the **same** buffer that is currently being used to render a frame, you will encounter a data race and weird behavior. You can reuse _some_ of the same stuff between frames

Example:

Rendering flow: Build frame info --> Render onto image --> Present image

Building frame 1 info: [0..2]
Rendering frame 1: [2..10]
Presenting 1 to screen: [10..26]

// Because on timings [2..26] the CPU is not doing anything, we're going to start building the next frame's info at [2].
Building frame 2 info: [2..4] // Here if we modify the same buffer that is being used to render frame 1, you will get some weird behavior.
Rendering frame 2: [4..12]
Presenting frame 2: [26..42]
