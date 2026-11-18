use std::cell::RefCell;
use std::rc::{self, Rc};
use std::sync::{self, Arc};

struct Node;

// Flagged: struct fields hold `Rc`, `Arc` and both `Weak`s, including nested in another type.
struct Tree {
    root: Rc<Node>,
    shared: Arc<Node>,
    parent: rc::Weak<Node>,
    observers: Vec<sync::Weak<Node>>,
}

// Flagged: an enum variant's field holds an `Rc`.
enum Link {
    Shared(Rc<RefCell<Node>>),
    Empty,
}

// Flagged: a type alias names an `Arc`, so its uses don't hide the pointer.
type SharedNode = Arc<Node>;

// Flagged: a parameter and a return type.
fn share(node: Rc<Node>) -> Arc<Node> {
    drop(node);
    Arc::new(Node)
}

impl Tree {
    // Flagged: an inherent method's receiver and return type.
    fn root(self: Rc<Self>) -> Option<Rc<Node>> {
        Some(Rc::clone(&self.root))
    }

    // Not flagged: a body that uses `Rc` keeps it out of the signature.
    fn count(&self) -> usize {
        let root = Rc::clone(&self.root);
        Rc::strong_count(&root)
    }
}

trait Shared {
    // Flagged: a trait declares the signature its impls must follow.
    fn shared(&self) -> Arc<Node>;
}

// Not flagged: a trait impl takes its signature from the trait.
impl Shared for Node {
    fn shared(&self) -> Arc<Node> {
        Arc::new(Node)
    }
}

// Not flagged: a borrow, a `Box` and a local `Rc` in the body.
struct Owner<'tree> {
    tree: &'tree Tree,
    node: Box<Node>,
}

fn lend(owner: &Owner<'_>) -> usize {
    let local = Rc::new(Node);
    owner.tree.count() + Rc::strong_count(&local)
}

// Not flagged: an exception that says why.
struct Cache {
    #[expect(shared_ownership, reason = "every window reads the same cache")]
    entries: Arc<Node>,
}

macro_rules! shared_field {
    ($name:ident) => {
        struct $name {
            node: std::rc::Rc<Node>,
        }
    };
}

// Not flagged: the field comes from a macro expansion.
shared_field!(Generated);

fn main() {
    let _ = (share, lend, Tree::root, Link::Empty);
}

// Not reported: flagged, but an item-level `expect` in the documented form takes it.
struct Excused {
    #[cfg_attr(
        dylint_lib = "trellis_lints",
        expect(shared_ownership, reason = "the fixture checks an item-level expect")
    )]
    entries: Arc<Node>,
}

// Flagged: statics and constants, associated ones included, name the pointer in their types.
static SHARED: Option<Arc<Node>> = None;

const EMPTY: Option<Rc<Node>> = None;

trait Defaults {
    const DEFAULT: Option<Rc<Node>>;
}

impl Node {
    const NONE: Option<Arc<Node>> = None;
}

// Not flagged: a trait impl's constant takes its type from the trait.
impl Defaults for Tree {
    const DEFAULT: Option<Rc<Node>> = None;
}

// Flagged: the same code with a raw identifier, whose text `is_from_proc_macro` misreads; only
// the code a derive writes is skipped for that.
struct RawShared {
    shared: r#Rc<u8>,
}
