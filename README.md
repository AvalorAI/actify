# actify

**Note that this crate is under construction. Although used in production, work is done on making an intuitive API, documententation and remaining features. For the time being, this does not follow semantic versioning!**

The [actor model](https://en.wikipedia.org/wiki/Actor_model) allows [atomic](https://www.codingem.com/atomic-meaning-in-programming/) access to a single instance of a data type.

Each actor holds an arbitrary inner data type, which can be accessed through clonable thread-safe handles.

Generic methods are get() and set(), but for the inner data type Vec and Hashmap additional methods are provided through the MapHandle and VecHandle trait.

Note that updating an Actor value through sequentially getting, updating and setting breaks any guarantees on atomicity. Hence the eval() method is added, which allows to remote execute a function **within** an actor. A drawback is that all arguments must be cast to a single [any type](https://doc.rust-lang.org/std/any/index.html), which prevents the compiler to do type-checking.

Additionally, a cache is provided, which can be used to locally synchronize with an actor.

**Feature request**

- General broadcast function from actor, instead of trait impls
- Macro for implementing type-specific actor functions
- Drain & Swap functions
