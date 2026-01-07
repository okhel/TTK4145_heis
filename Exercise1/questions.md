Exercise 1 - Theory questions
-----------------------------

### Concepts

What is the difference between *concurrency* and *parallelism*?
> parallelism tasks are completely separated, with concurrency you can share things like data. 

What is the difference between a *race condition* and a *data race*? 
> race condition: when timing of the program affects what output you get, data race: several threads are trying to manipulate the same simultaneously 
 
*Very* roughly - what does a *scheduler* do, and how does it do it?
> Decides which thread gets a certain resource. Does it by having a queue of blocked threads, available threads, waiting/asleep


### Engineering

Why would we use multiple threads? What kinds of problems do threads solve?
> can achieve speedups by doing tasks at the same time. Solves problems where you can do useful work while idling in other tasks.

Some languages support "fibers" (sometimes called "green threads") or "coroutines"? What are they, and why would we rather use them over threads?
> fibers are cooperative threads. They always start and stop in well-defined places, preventing a thread stopping in the middle because of a timer

Does creating concurrent programs make the programmer's life easier? Harder? Maybe both?
> Harder (more complicated), but more performant program

What do you think is best - *shared variables* or *message passing*?
> message passing, because it is easy to make mistakes in shared variables, e.g. two threads working on the same data without sync. 


