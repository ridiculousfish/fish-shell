use crate::env::EnvStack;
use crate::input::{input_mappings, InputEventMapper, KeyNameStyle, DEFAULT_BIND_MODE};
use crate::input_common::{CharEvent, InputData, InputEventQueuer, ReadlineCmd};
use crate::key::Key;
use crate::tests::prelude::*;
use crate::wchar::prelude::*;
use std::rc::Rc;

struct TestInputEventMapper {
    input_data: InputData,
    vars: Rc<EnvStack>,
}

impl InputEventQueuer for TestInputEventMapper {
    fn get_input_data(&self) -> &InputData {
        &self.input_data
    }
    fn get_input_data_mut(&mut self) -> &mut InputData {
        &mut self.input_data
    }
}

impl InputEventMapper for TestInputEventMapper {
    fn get_vars(&self) -> Rc<EnvStack> {
        self.vars.clone()
    }
}

#[test]
#[serial]
fn test_input() {
    let _cleanup = test_init();
    use crate::env::EnvStack;
    let mut input = TestInputEventMapper {
        input_data: InputData::new(libc::STDIN_FILENO),
        vars: Rc::new(EnvStack::new()),
    };
    // Ensure sequences are order independent. Here we add two bindings where the first is a prefix
    // of the second, and then emit the second key list. The second binding should be invoked, not
    // the first!
    let prefix_binding: Vec<Key> = "qqqqqqqa".chars().map(Key::from_raw).collect();
    let mut desired_binding = prefix_binding.clone();
    desired_binding.push(Key::from_raw('a'));

    let default_mode = || DEFAULT_BIND_MODE.to_owned();

    {
        let mut input_mapping = input_mappings();
        input_mapping.add1(
            prefix_binding,
            KeyNameStyle::Plain,
            WString::from_str("up-line"),
            default_mode(),
            None,
            true,
        );
        input_mapping.add1(
            desired_binding.clone(),
            KeyNameStyle::Plain,
            WString::from_str("down-line"),
            default_mode(),
            None,
            true,
        );
    }

    // Push the desired binding to the queue.
    for c in desired_binding {
        input.input_data.queue_char(CharEvent::from_key(c));
    }

    // Now test.
    let evt = input.read_char();
    if !evt.is_readline() {
        panic!("Event is not a readline");
    } else if evt.get_readline() != ReadlineCmd::DownLine {
        panic!("Expected to read char down_line");
    }
}
