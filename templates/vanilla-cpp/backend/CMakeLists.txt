cmake_minimum_required(VERSION 3.16)
project(todo_server CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# put the executable into build/bin for every generator (incl. multi-config Visual Studio)
set(CMAKE_RUNTIME_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/bin)
foreach(config Debug Release RelWithDebInfo MinSizeRel)
  string(TOUPPER ${config} CONFIG)
  set(CMAKE_RUNTIME_OUTPUT_DIRECTORY_${CONFIG} ${CMAKE_BINARY_DIR}/bin)
endforeach()

add_executable(todo-server src/main.cpp)

if(WIN32)
  target_link_libraries(todo-server PRIVATE ws2_32)
endif()
