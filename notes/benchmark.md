## Benchark log

#### Creating an actual benchmarker

Starting the window, the engine and then looking at frame time is not a great idea for benchmarking. This is because to test different angles, setups would mean the steps of running the app, wait for the window to appear, move to a specific position in the world would take a long time between retries. But also, the setups could be incosistent and tedious. However, benchmarking this game engine is not a simple task.

There are two options that present ourselves that have various pros and cons:

1. Create my own benchmarker
   - Pros:
     - Don't have to rewrite code
     - Can view the tests while they are running!
     - Creating a benchmarking app would be kinda cool
   - Cons:
     - Would have to transform the Application part of the code into having a whole benchmarking suite
2. Use an external library
   - Pros:
     - Don't have to write an entire library to benchmark
     - Could use cirterion that can use statistics, plots and "more rigorous" benchmarking
     - Would decouple the code into making the entire engine run as a headless application rendering to an image
     - The benchmarking part of the engine would be decoupled from the application
     - Running benchmarks would be less tedious
   - Cons:
     - Rewriting a lot of code

#### Precomputing camera viewport

- Compute shader time: 0.74ms (20% improvement!)
- Copy to present image time: 0.26ms

This is the thing that I (and chat gpt but I thought of it first) would make the biggest difference. Its the same computation over and over again for each pixel that we cut out the viewport calculation into the cpu once and reuse them each time. We only recalculate on the resizing of the image and when we move the camera.

#### branchless checking for delta t

- from 0.94ms to 0.93ms

#### Expanding casts between ve4 and vec3

- no difference.

#### Installing nvidia drivers

- Compute shader time: 0.92ms
- Copy to present image time: 0.26ms

Debian uses `nouveau` which are open source nvidia drivers. After installing the nvidia drivers the times just decresed and are more consistant. They did say that the nouveau drivers are slower but gdaaam these are fast. I didn't event need to optimize that much. I do wonder if I could optimize some more by changing the shader code. But this seems like overkill.

After writing out just a white image. It seems like the minimum time for rendering a compute shader is ~0.3ms. This means that I can definitely optimize the shader code a lot. This is great! This means that I have so much more room to code an actually ray tracer on voxels!

#### Adding the UI

- Compute shader time: ~5ms
- Copy to present image time: ~1.6ms

This is surprising since I haven't changed anyting besides adding a GUI. The timings are calculatd per swapchain image for the compute and copy operations and do not include the GUI which is why I am surprised.

The only thing that can make a difference is that the swapchain images now use R16B16G16A16_SFLOAT by default (the first format that appears on the surface capabilities) and it takes 0.6 ms more to copy the image. This would make sense but even without, the copy operation take ~1ms and not 0.96ms. Why is is it slower?

Cool thing though, changing the shader does have a real effect on performance. One division and multiplication less had 0.4 ms less on average!

#### Rendering a 10x10 cube in the middle of the screen

- Compute shader time: 2.88009925456032 ms
- Copy to present image time: 0.96153502934372 ms

This is very surprising. Since this is the base case and I haven't done any cool traversal yet, I would imagine that this would take less than 1 ms. However, even rendering a simple color image to the screen takes ~1 ms.
