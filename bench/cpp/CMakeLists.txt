cmake_minimum_required(VERSION 3.16)

# Headless benchmark harness for the unmodified Tablecruncher core (see bench/README.md).
# Configure like the main app: -DFLTKDIR=... or -DFLTKINCDIR=... -DFLTKLIBDIR=...
project(tc_bench_cpp C CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
if(NOT CMAKE_BUILD_TYPE)
    set(CMAKE_BUILD_TYPE Release)
endif()
set(CMAKE_CXX_FLAGS_RELEASE "-O2")      # same optimisation level as the app

set(FLTKDIR "" CACHE PATH "Optional root path to FLTK")
set(FLTKINCDIR "" CACHE PATH "Path to FLTK include directory")
set(FLTKLIBDIR "" CACHE PATH "Path to FLTK lib/build directory")
if(NOT FLTKINCDIR)
    set(FLTKINCDIR ${FLTKDIR})
endif()
if(NOT FLTKLIBDIR)
    set(FLTKLIBDIR ${FLTKDIR})
endif()
if(NOT FLTKINCDIR OR NOT FLTKLIBDIR)
    message(FATAL_ERROR "Please set FLTKDIR (or FLTKINCDIR and FLTKLIBDIR).")
endif()

set(SRC ${CMAKE_CURRENT_SOURCE_DIR}/../../src)
set(EXT ${CMAKE_CURRENT_SOURCE_DIR}/../../external)

add_executable(tc_bench_cpp
    tc_bench_cpp.cpp
    ${SRC}/csvparser.cpp
    ${SRC}/csvdatastorage.cpp
    ${SRC}/csvtable.cpp
    ${SRC}/helper.cpp
    ${SRC}/macro.cpp
    ${EXT}/duktape/duktape.c
)
# Removes the parser's status-bar updates (its only GUI dependency).
target_compile_definitions(tc_bench_cpp PRIVATE TCRUNCHER_PARSER_WITHOUT_UPDATE)
target_include_directories(tc_bench_cpp PRIVATE ${SRC} ${EXT} ${FLTKINCDIR} ${FLTKLIBDIR})

# FLTK is linked only because CsvDataStorage::sort() calls Fl::check().
if(WIN32)
    target_compile_definitions(tc_bench_cpp PRIVATE WIN32_LEAN_AND_MEAN NOMINMAX _FILE_OFFSET_BITS=64)
    target_compile_options(tc_bench_cpp PRIVATE $<$<COMPILE_LANGUAGE:CXX>:/utf-8>)
    set(FLTK_DIR ${FLTKLIBDIR})
    find_package(FLTK 1.4 CONFIG REQUIRED)
    target_link_libraries(tc_bench_cpp PRIVATE fltk::fltk psapi)
elseif(APPLE)
    target_link_libraries(tc_bench_cpp PRIVATE
        ${FLTKLIBDIR}/lib/libfltk.a
        "-framework Cocoa"
        "-framework CoreFoundation"
    )
else()
    target_link_libraries(tc_bench_cpp PRIVATE
        ${FLTKLIBDIR}/lib/libfltk.a
        Xft fontconfig Xrender X11 pthread dl
    )
endif()
