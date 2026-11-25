//! Last activity per subtree, and Quiet. Added in a later step.

use crate::compute::{Ctx, Entry, Wake};

pub(crate) fn quiet(_ctx: &Ctx<'_>, _out: &mut Vec<Entry>, _wake: &mut Wake) {}
