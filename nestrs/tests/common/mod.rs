pub struct RegistryResetGuard;

impl RegistryResetGuard {
    pub fn new() -> Self {
        #[cfg(feature = "test-hooks")]
        {
            nestrs::core::RouteRegistry::clear_for_tests();
            nestrs::core::MetadataRegistry::clear_for_tests();
        }
        Self
    }
}

impl Drop for RegistryResetGuard {
    fn drop(&mut self) {
        #[cfg(feature = "test-hooks")]
        {
            nestrs::core::RouteRegistry::clear_for_tests();
            nestrs::core::MetadataRegistry::clear_for_tests();
        }
    }
}
