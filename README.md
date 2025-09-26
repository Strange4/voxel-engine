# TODO

### Make a moving camera
Push constants the origin and direction of the camera.


## Questions and answers.

#### What is a stencil image?
When you are making a grafiti, you use a stencil to "mask" the shape that you want to draw on. Imagine it like letters that are being painted on a cargo container.
A stencil image in vulkan is an image that is used like a mask to draw other images. Like a stencil in real life.

#### What are descriptor sets?

A descriptor is an object that "describes" some resource used by a shader pipeline. For example, a descriptor for an image could describe the color type, the size and other factors.
When you pass those resources to the pipeline they aren't passed 1 by 1, they are grouped into descriptor sets and then the whole set is passed to the pipeline to be used. Then each element of that set has a "binding" that can be bound to a variable in the shader.


###### What are descriptor sets layouts?

Read up on previous question. The layout of a descriptor set is an array of all the bindings of the descriptor sets and they type.


# DONE
[x] Create a 3D texture for tracing voxels

