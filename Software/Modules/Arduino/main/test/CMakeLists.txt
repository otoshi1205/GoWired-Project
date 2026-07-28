cmake_minimum_required(VERSION 3.14)
project(gowired_module_tests CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

include(FetchContent)
FetchContent_Declare(
  googletest
  URL https://github.com/google/googletest/archive/refs/tags/v1.14.0.zip
)
FetchContent_MakeAvailable(googletest)

# The domain layer builds on the host untouched: it includes only src/hal and
# src/domain, and neither pulls in <Arduino.h>. That is the whole point of the
# HAL split -- if a file here ever fails to compile because it wants an Arduino
# header, the layering has been broken.
set(DOMAIN_DIR ${CMAKE_CURRENT_SOURCE_DIR}/../src/domain)

add_library(gowired_domain STATIC
  ${DOMAIN_DIR}/input.cpp
  ${DOMAIN_DIR}/monitors.cpp
  ${DOMAIN_DIR}/shutter.cpp
  ${DOMAIN_DIR}/dimmer.cpp
  ${DOMAIN_DIR}/input_bank.cpp
  ${DOMAIN_DIR}/relay_bank_device.cpp
  ${DOMAIN_DIR}/roller_shutter_device.cpp
  ${DOMAIN_DIR}/dimmer_device.cpp
  ${DOMAIN_DIR}/module.cpp
)
target_include_directories(gowired_domain PUBLIC ${CMAKE_CURRENT_SOURCE_DIR}/../src)
target_compile_options(gowired_domain PRIVATE -Wall -Wextra -Werror)

enable_testing()

add_executable(gowired_tests
  config_test.cpp
  input_test.cpp
  monitors_test.cpp
  shutter_test.cpp
  dimmer_test.cpp
  input_bank_test.cpp
  relay_bank_device_test.cpp
  dimmer_device_test.cpp
  roller_shutter_device_test.cpp
  module_test.cpp
)
target_include_directories(gowired_tests PRIVATE ${CMAKE_CURRENT_SOURCE_DIR})
target_link_libraries(gowired_tests PRIVATE gowired_domain GTest::gtest_main)
target_compile_options(gowired_tests PRIVATE -Wall -Wextra)

include(GoogleTest)
gtest_discover_tests(gowired_tests)
